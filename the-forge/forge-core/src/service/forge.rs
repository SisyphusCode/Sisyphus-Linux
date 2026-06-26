use forge_common::{BootProfileReport, ServiceStatus, WaveTiming};
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use parking_lot::{Mutex, MutexGuard};
use serde::Deserialize;
use slotmap::SlotMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::chown;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn resolve_user_to_uid(user: &str) -> Option<u32> {
    use std::ffi::CString;
    let cname = CString::new(user).ok()?;
    unsafe {
        let pwd = libc::getpwnam(cname.as_ptr());
        if pwd.is_null() {
            None
        } else {
            Some((*pwd).pw_uid)
        }
    }
}

fn resolve_group_to_gid(group: &str) -> Option<u32> {
    use std::ffi::CString;
    let cname = CString::new(group).ok()?;
    unsafe {
        let grp = libc::getgrnam(cname.as_ptr());
        if grp.is_null() {
            None
        } else {
            Some((*grp).gr_gid)
        }
    }
}

const RESTART_BURST_MAX: usize = 5;
const RESTART_BURST_WINDOW: Duration = Duration::from_secs(60);

use super::ghosttype_log;
use super::manifest::{DeviceManifest, MountManifest, ServiceManifest, ServiceType, TimerManifest};

fn oneshot_timeout() -> Duration {
    let secs = std::env::var("FORGE_ONESHOT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    Duration::from_secs(secs)
}

fn parse_signal(s: &str) -> Signal {
    let trimmed = s.trim();
    // Try as-is (e.g. "SIGTERM")
    if let Ok(sig) = trimmed.parse::<Signal>() {
        return sig;
    }
    let upper = trimmed.to_ascii_uppercase();
    if let Ok(sig) = upper.parse::<Signal>() {
        return sig;
    }
    // Bare name without SIG prefix, e.g. "TERM"
    if !upper.starts_with("SIG") {
        if let Ok(sig) = format!("SIG{}", upper).parse::<Signal>() {
            return sig;
        }
    }
    // Numeric signal number, e.g. "15" or "9"
    if let Ok(num) = trimmed.parse::<i32>() {
        if let Ok(sig) = Signal::try_from(num) {
            return sig;
        }
    }
    Signal::SIGHUP
}

use super::socket::{activate_sockets, SocketActivation};
use crate::cgroup::{self, ServiceLimits};
use crate::environment::merge_environment;
use crate::notify::{parse_notify, prepare_notify_socket, wait_for_ready, NotifyMessage};

slotmap::new_key_type! { pub struct UnitKey; }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitState {
    Dead,
    Starting,
    Listening,
    Running,
    Failed,
    Stopping,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    #[default]
    No,
    OnFailure,
    Always,
}

#[derive(Debug)]
pub enum UnitKind {
    Service {
        exec: String,
        args: Vec<String>,
        after: Vec<String>,
        requires: Vec<String>,
        wants: Vec<String>,
        sockets: Vec<String>,
        restart: RestartPolicy,
        cgroup: ServiceLimits,
        service_type: ServiceType,
        environment: Vec<(String, String)>,
        exec_start_pre: Option<(String, Vec<String>)>,
        bus_name: Option<String>,
        sandbox: crate::sandbox::SandboxConfig,
        user: Option<String>,
        group: Option<String>,
        working_directory: Option<String>,
        runlevels: Vec<String>,
        timeout_secs: Option<u64>,
        pidfile: Option<String>,
        pid: Option<u32>,
        /// WATCHDOG_USEC (in microseconds) if the service uses sd_notify watchdog.
        watchdog_usec: Option<u64>,
        last_watchdog: Option<Instant>,
        /// Custom stop command; takes precedence over `stop_signal`.
        stop_cmd: Option<(String, Vec<String>)>,
        /// Signal name to send on graceful stop. Default: SIGTERM.
        stop_signal: Option<String>,
        /// Custom reload command; runs without stopping the service.
        reload_cmd: Option<(String, Vec<String>)>,
        /// Signal name to send on reload. Default: SIGHUP.
        reload_signal: Option<String>,
    },
    Timer {
        unit: String,
        after: Vec<String>,
        on_boot_sec: Option<f64>,
        fired: bool,
    },
    Mount {
        what: String,
        where_: String,
        fstype: String,
        options: String,
        after: Vec<String>,
        mounted: bool,
    },
    Device {
        path: PathBuf,
        service: String,
        after: Vec<String>,
        activated: bool,
    },
    Socket {
        listen: Vec<String>,
        service: Option<String>,
        after: Vec<String>,
        activation: Option<SocketActivation>,
    },
    Target {
        requires: Vec<String>,
        wants: Vec<String>,
    },
}

pub struct Unit {
    pub name: Arc<str>,
    pub kind: UnitKind,
    pub state: UnitState,
    pub dependencies: Vec<UnitKey>,
    pub last_exit: Option<i32>,
}

pub struct ForgeState {
    arena: SlotMap<UnitKey, Unit>,
    name_index: HashMap<Arc<str>, UnitKey>,
    pid_index: HashMap<u32, UnitKey>,
    /// Maps virtual provider names (from `provide = [...]`) to the first unit that claimed them.
    provides_index: HashMap<String, UnitKey>,
    active_units: HashSet<UnitKey>,
    pub active_target: String,
    boot_profile: BootProfileReport,
    boot_started: Instant,
    log_dir: PathBuf,
    shutting_down: bool,
    restart_times: HashMap<String, Vec<Instant>>,
    boot_lenient: bool,
    strict_boot: bool,
    required_units: HashSet<UnitKey>,
    /// Reverse dependencies: for a unit, the list of units that depend on it (populated during resolve).
    reverse_dependencies: HashMap<UnitKey, Vec<UnitKey>>,
    pending_restarts: VecDeque<UnitKey>,
    job_queue: crate::jobs::JobQueue,
}

#[derive(Debug)]
pub enum ForgeError {
    #[allow(dead_code)]
    DuplicateName(String),
    MissingUnit {
        unit: String,
        dependency: String,
    },
    CycleDetected,
    SpawnFailed {
        name: String,
        error: String,
    },
    SocketFailed {
        name: String,
        error: String,
    },
    NotFound(String),
    NotService(String),
    NotRunning(String),
    InvalidOperation(String),
}

impl std::fmt::Display for ForgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateName(name) => write!(f, "duplicate unit name '{name}'"),
            Self::MissingUnit { unit, dependency } => {
                write!(f, "unit '{unit}' references unknown '{dependency}'")
            }
            Self::CycleDetected => write!(f, "dependency cycle detected"),
            Self::SpawnFailed { name, error } => write!(f, "failed to start '{name}': {error}"),
            Self::SocketFailed { name, error } => write!(f, "socket '{name}' failed: {error}"),
            Self::NotFound(name) => write!(f, "unit '{name}' not found"),
            Self::NotService(name) => write!(f, "'{name}' is not a service unit"),
            Self::NotRunning(name) => write!(f, "service '{name}' is not running"),
            Self::InvalidOperation(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ForgeError {}

impl ForgeState {
    pub fn new(active_target: String, log_dir: PathBuf) -> Self {
        Self {
            arena: SlotMap::with_key(),
            name_index: HashMap::new(),
            pid_index: HashMap::new(),
            provides_index: HashMap::new(),
            active_units: HashSet::new(),
            active_target: active_target.clone(),
            boot_profile: BootProfileReport {
                total_boot_ms: 0,
                active_target,
                waves: Vec::new(),
            },
            boot_started: Instant::now(),
            log_dir,
            shutting_down: false,
            restart_times: HashMap::new(),
            boot_lenient: false,
            strict_boot: false,
            required_units: HashSet::new(),
            reverse_dependencies: HashMap::new(),
            pending_restarts: VecDeque::new(),
            job_queue: crate::jobs::JobQueue::new(),
        }
    }

    pub fn enqueue_job(&mut self, unit: &str, mode: &str) -> crate::jobs::Job {
        self.job_queue.enqueue(unit, mode)
    }

    pub fn finish_job(&mut self, id: u32, result: crate::jobs::JobResult) {
        if let Some(job) = self.job_queue.finish(id, result) {
            ghosttype_log(
                "JOB",
                &format!(
                    "Job {} {} finished ({:?})",
                    job.id,
                    job.unit,
                    job.result.unwrap_or(crate::jobs::JobResult::Done)
                ),
            );
        }
    }

    pub fn list_units_dbus(
        &self,
    ) -> Vec<(
        String,
        String,
        String,
        String,
        u32,
        String,
        String,
        u32,
        bool,
        bool,
    )> {
        self.arena
            .iter()
            .map(|(_, u)| {
                let state = match u.state {
                    UnitState::Running | UnitState::Listening => "active",
                    UnitState::Failed => "failed",
                    UnitState::Starting => "activating",
                    _ => "inactive",
                };
                (
                    format!("{}.service", u.name),
                    state.into(),
                    state.into(),
                    String::new(),
                    0u32,
                    self.active_target.clone(),
                    format!("{}.service", u.name),
                    0u32,
                    false,
                    false,
                )
            })
            .collect()
    }

    pub fn attach_session_scope(&mut self, name: &str, leader: Option<u32>) {
        let cgroup = format!("/sys/fs/cgroup/{name}");
        let _ = std::fs::create_dir_all(&cgroup);
        if let Some(pid) = leader {
            if let Ok(mut fh) = OpenOptions::new()
                .write(true)
                .open(format!("{cgroup}/cgroup.procs"))
            {
                let _ = writeln!(fh, "{pid}");
                ghosttype_log(
                    "SCOPE",
                    &format!("Attached leader pid {pid} to session scope '{name}'"),
                );
                return;
            }
            ghosttype_log(
                "SCOPE",
                &format!("Failed to attach leader pid {pid} to '{name}'"),
            );
        }
        ghosttype_log("SCOPE", &format!("Registered session scope '{name}'"));
    }

    pub fn start_user_manager_stub(&mut self, unit: &str) -> Result<(), ForgeError> {
        let uid = unit
            .split('@')
            .nth(1)
            .and_then(|s| s.strip_suffix(".service"))
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| ForgeError::InvalidOperation(format!("bad user unit '{unit}'")))?;
        let runtime = format!("/run/user/{uid}");
        let _ = std::fs::create_dir_all(&runtime);
        let _ = chown(&runtime, Some(uid), None);
        let bus = format!("{runtime}/bus");
        let session_addr = format!("unix:path={bus}");
        if !std::path::Path::new(&bus).exists() {
            let _ = std::process::Command::new("/usr/bin/dbus-daemon")
                .args([
                    "--session",
                    &format!("--address=unix:path={bus}"),
                    "--nopidfile",
                    "--nofork",
                ])
                .env("XDG_RUNTIME_DIR", &runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &session_addr)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            let _ = std::process::Command::new(
                "/usr/libexec/forge/systemd1-session-stub-wrapper.sh",
            )
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("DBUS_SESSION_BUS_ADDRESS", &session_addr)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        }
        ghosttype_log("USER", &format!("user@{uid}.service session bus at {bus}"));
        Ok(())
    }

    pub fn set_boot_lenient(&mut self, lenient: bool) {
        self.boot_lenient = lenient;
    }

    #[allow(dead_code)]
    pub fn boot_lenient(&self) -> bool {
        self.boot_lenient
    }

    /// When true (PID 1 production), `need`/`requires` failures abort the boot target.
    pub fn set_strict_boot(&mut self, strict: bool) {
        self.strict_boot = strict;
    }

    fn stop_timeout() -> Duration {
        let secs = std::env::var("FORGE_STOP_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(90);
        Duration::from_secs(secs)
    }

    pub fn persist_softlevel(&self, target: &str) {
        let _ = std::fs::create_dir_all("/run/forge");
        let _ = std::fs::create_dir_all("/var/lib/forge");
        let _ = std::fs::write("/run/forge/softlevel", target);
        let _ = std::fs::write("/var/lib/forge/softlevel", target);
    }

    fn host_process_running(name: &str) -> bool {
        if Command::new("pgrep")
            .arg("-x")
            .arg(name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return true;
        }
        // Comm names are truncated to 15 bytes; fall back to cmdline match.
        Command::new("pgrep")
            .arg("-f")
            .arg(name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Services that must never be started while a desktop session is already running.
    fn sandbox_skip_reason(name: &str) -> Option<&'static str> {
        match name {
            "display-manager" | "forge-desktop" => {
                Some("will not start desktop session on a live system (logs you out)")
            }
            "getty-tty3" | "getty-console" => {
                Some("will not attach agetty while desktop is active")
            }
            "network-setup" => Some("defer network bring-up to host NetworkManager"),
            "udev-trigger" => Some("will not re-trigger udev on a live system"),
            "udev-settle" => Some("will not settle udev on a live system"),
            // forge-early.sh uses pkill — never run it on a live desktop (kills GNOME logind).
            "forge-early" => {
                Some("forge-early uses pkill — use forge-mock-boot.sh for safe simulation")
            }
            "user-sessions" => Some("systemd-user-sessions mutates host login policy"),
            "NetworkManager" => Some("will not start a second NetworkManager on a live system"),
            _ => None,
        }
    }

    fn adopt_host_service(&mut self, key: UnitKey, detail: &str) -> Result<u32, ForgeError> {
        let name = self.arena[key].name.clone();
        self.arena[key].state = UnitState::Running;
        ghosttype_log("SANDBOX", &format!("'{name}' — {detail}"));
        crate::journal::record(&name, 6, detail.to_string(), None);
        Ok(0)
    }

    fn open_service_log(&mut self, name: &str) -> Result<(std::fs::File, PathBuf), ForgeError> {
        let primary = self.log_dir.join(format!("{name}.log"));
        if let Some(parent) = primary.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&primary) {
            return Ok((file, primary));
        }

        let fallback_dir = PathBuf::from("/run/forge/log");
        let _ = fs::create_dir_all(&fallback_dir);
        self.log_dir = fallback_dir.clone();
        let fallback = fallback_dir.join(format!("{name}.log"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&fallback)
            .map_err(|e| ForgeError::SpawnFailed {
                name: name.to_string(),
                error: e.to_string(),
            })?;
        ghosttype_log(
            "RECOVERY",
            &format!(
                "Service '{name}' logging to {} (primary log dir not writable)",
                fallback.display()
            ),
        );
        Ok((file, fallback))
    }

    fn restart_allowed(&mut self, name: &str) -> bool {
        let now = Instant::now();
        let times = self.restart_times.entry(name.to_string()).or_default();
        times.retain(|t| now.duration_since(*t) < RESTART_BURST_WINDOW);
        if times.len() >= RESTART_BURST_MAX {
            ghosttype_log(
                "RESTART",
                &format!("'{name}' hit restart burst limit ({RESTART_BURST_MAX}/{RESTART_BURST_WINDOW:?}) — holding"),
            );
            return false;
        }
        times.push(now);
        true
    }

    /// Update last_watchdog / timeout from a received notify message (called from activation or future drain).
    #[allow(dead_code)]
    pub fn update_watchdog(&mut self, key: UnitKey, parsed: &NotifyMessage) {
        if let UnitKind::Service {
            watchdog_usec,
            last_watchdog,
            ..
        } = &mut self.arena[key].kind
        {
            if let Some(us) = parsed.watchdog_usec {
                *watchdog_usec = Some(us);
            }
            if parsed.watchdog {
                *last_watchdog = Some(Instant::now());
                if let Some(us) = *watchdog_usec {
                    ghosttype_log(
                        "WATCHDOG",
                        &format!("ping for '{}' ({} usec)", self.arena[key].name, us),
                    );
                }
            }
        }
    }

    /// Check services with active watchdogs. Restart those that missed their deadline.
    /// Called from reaping paths.
    pub fn check_watchdogs(&mut self) {
        let now = Instant::now();
        let overdue: Vec<UnitKey> = self
            .arena
            .iter()
            .filter_map(|(k, u)| {
                if let UnitKind::Service {
                    pid: Some(_),
                    watchdog_usec: Some(us),
                    last_watchdog: Some(last),
                    ..
                } = &u.kind
                {
                    let deadline = *last + Duration::from_micros(*us);
                    if now > deadline {
                        return Some(k);
                    }
                }
                None
            })
            .collect();

        for key in overdue {
            let name = self.arena[key].name.clone();
            ghosttype_log(
                "WATCHDOG",
                &format!("'{}' missed watchdog — restarting", name),
            );
            // Force a restart by stopping then queuing
            let _ = self.stop_service(key, Signal::SIGTERM);
            self.pending_restarts.push_back(key);
        }
    }

    pub fn len(&self) -> usize {
        self.arena.len()
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    pub fn boot_profile(&self) -> &BootProfileReport {
        &self.boot_profile
    }

    pub fn log_dir(&self) -> &PathBuf {
        &self.log_dir
    }

    #[allow(dead_code)]
    pub fn set_log_dir(&mut self, dir: PathBuf) {
        self.log_dir = dir;
    }

    pub fn unit_failed(&self, name: &str) -> bool {
        self.name_index
            .get(name)
            .is_some_and(|k| self.arena[*k].state == UnitState::Failed)
    }

    pub fn unit_running(&self, name: &str) -> bool {
        self.name_index
            .get(name)
            .is_some_and(|k| self.arena[*k].state == UnitState::Running)
    }

    pub fn boot_health_signals(&self) -> (bool, bool, bool) {
        let dbus_ok = self.unit_running("dbus")
            || std::path::Path::new("/run/dbus/system_bus_socket").exists();
        let getty_ok = self.unit_running("getty-tty3") || self.unit_running("getty-console");
        let critical_failed = ["forge-early", "dbus"].iter().any(|n| self.unit_failed(n));
        (dbus_ok, getty_ok, critical_failed)
    }

    pub fn register_service_manifest(&mut self, m: ServiceManifest) -> Result<UnitKey, ForgeError> {
        let provides = m.provides.clone();
        let svc_name = m.name.clone();
        let key = self.insert_unit(
            m.name.clone(),
            UnitKind::Service {
                exec: m.exec,
                args: m.args,
                after: m.after,
                requires: m.requires,
                wants: m.wants,
                sockets: m.sockets,
                restart: m.restart,
                cgroup: m.cgroup,
                service_type: m.service_type,
                environment: m.environment,
                exec_start_pre: m.exec_start_pre,
                bus_name: m.bus_name,
                sandbox: m.sandbox,
                user: m.user,
                group: m.group,
                working_directory: m.working_directory,
                runlevels: m.runlevels,
                timeout_secs: m.timeout_secs,
                pidfile: m.pidfile,
                pid: None,
                watchdog_usec: m.watchdog_usec,
                last_watchdog: None,
                stop_cmd: m.stop_cmd,
                stop_signal: m.stop_signal,
                reload_cmd: m.reload_cmd,
                reload_signal: m.reload_signal,
            },
        )?;
        // Register virtual provider names; first-registered unit wins (OpenRC semantics).
        for vname in provides {
            let entry = self.provides_index.entry(vname.clone()).or_insert(key);
            if *entry == key {
                ghosttype_log(
                    "PROVIDE",
                    &format!("'{svc_name}' provides virtual '{vname}'"),
                );
            } else {
                ghosttype_log(
                    "PROVIDE",
                    &format!("Virtual '{vname}' already claimed — '{svc_name}' is an alternative provider"),
                );
            }
        }
        Ok(key)
    }

    pub fn register_timer(&mut self, m: TimerManifest) -> Result<UnitKey, ForgeError> {
        self.insert_unit(
            m.name.clone(),
            UnitKind::Timer {
                unit: m.unit,
                after: m.after,
                on_boot_sec: m.on_boot_sec,
                fired: false,
            },
        )
    }

    pub fn register_mount(&mut self, m: MountManifest) -> Result<UnitKey, ForgeError> {
        self.insert_unit(
            m.name.clone(),
            UnitKind::Mount {
                what: m.what,
                where_: m.where_,
                fstype: m.fstype,
                options: m.options,
                after: m.after,
                mounted: false,
            },
        )
    }

    pub fn register_service(
        &mut self,
        name: String,
        exec: String,
        args: Vec<String>,
        after: Vec<String>,
        requires: Vec<String>,
        wants: Vec<String>,
        sockets: Vec<String>,
        restart: RestartPolicy,
        cgroup: ServiceLimits,
    ) -> Result<UnitKey, ForgeError> {
        let mut m = ServiceManifest::new(name, exec);
        m.args = args;
        m.after = after;
        m.requires = requires;
        m.wants = wants;
        m.sockets = sockets;
        m.restart = restart;
        m.cgroup = cgroup;
        self.register_service_manifest(m)
    }

    pub fn register_device(&mut self, m: DeviceManifest) -> Result<UnitKey, ForgeError> {
        self.insert_unit(
            m.name.clone(),
            UnitKind::Device {
                path: m.path,
                service: m.service,
                after: m.after,
                activated: false,
            },
        )
    }

    pub fn register_socket(
        &mut self,
        name: String,
        listen: Vec<String>,
        service: Option<String>,
        after: Vec<String>,
    ) -> Result<UnitKey, ForgeError> {
        self.insert_unit(
            name,
            UnitKind::Socket {
                listen,
                service,
                after,
                activation: None,
            },
        )
    }

    pub fn register_target(
        &mut self,
        name: String,
        requires: Vec<String>,
        wants: Vec<String>,
    ) -> Result<UnitKey, ForgeError> {
        self.insert_unit(name, UnitKind::Target { requires, wants })
    }

    /// OpenRC `before=` — ensure `must_start_first` is ordered ahead of `unit`.
    pub fn prepend_ordering(
        &mut self,
        unit: &str,
        must_start_first: &str,
    ) -> Result<(), ForgeError> {
        let key = self
            .name_index
            .get(unit)
            .copied()
            .ok_or_else(|| ForgeError::NotFound(unit.to_string()))?;
        let Some(unit_ref) = self.arena.get_mut(key) else {
            return Err(ForgeError::NotFound(unit.to_string()));
        };
        let dep = must_start_first.to_string();
        match &mut unit_ref.kind {
            UnitKind::Service { after, .. }
            | UnitKind::Socket { after, .. }
            | UnitKind::Timer { after, .. }
            | UnitKind::Mount { after, .. }
            | UnitKind::Device { after, .. } => {
                if !after.contains(&dep) {
                    after.push(dep);
                }
            }
            UnitKind::Target { .. } => {}
        }
        Ok(())
    }

    fn insert_unit(&mut self, name: String, kind: UnitKind) -> Result<UnitKey, ForgeError> {
        let name_arc: Arc<str> = name.into();
        if let Some(&existing) = self.name_index.get(&name_arc) {
            ghosttype_log(
                "WARN",
                &format!("Duplicate unit '{name_arc}' — keeping first definition"),
            );
            return Ok(existing);
        }
        let key = self.arena.insert(Unit {
            name: name_arc.clone(),
            kind,
            state: UnitState::Dead,
            dependencies: Vec::new(),
            last_exit: None,
        });
        self.name_index.insert(name_arc, key);
        Ok(key)
    }

    pub fn resolve_target_closure(&mut self) -> Result<(), ForgeError> {
        let target_name = self.active_target.clone();
        let target_key = self
            .name_index
            .get(target_name.as_str())
            .copied()
            .ok_or_else(|| ForgeError::MissingUnit {
                unit: "default-target".into(),
                dependency: target_name.clone(),
            })?;

        if !matches!(self.arena[target_key].kind, UnitKind::Target { .. }) {
            return Err(ForgeError::InvalidOperation(format!(
                "'{target_name}' is not a target unit"
            )));
        }

        let mut queue = VecDeque::from([target_key]);
        let mut seen = HashSet::new();

        while let Some(key) = queue.pop_front() {
            if !seen.insert(key) {
                continue;
            }

            let (requires, wants) = match &self.arena[key].kind {
                UnitKind::Target { requires, wants } => (requires.clone(), wants.clone()),
                UnitKind::Service {
                    requires, wants, ..
                } => (requires.clone(), wants.clone()),
                UnitKind::Socket { service, .. } => {
                    if let Some(svc) = service {
                        (vec![svc.clone()], Vec::new())
                    } else {
                        (Vec::new(), Vec::new())
                    }
                }
                UnitKind::Device { service, .. } => (Vec::new(), vec![service.clone()]),
                UnitKind::Timer { .. } => (Vec::new(), Vec::new()),
                UnitKind::Mount { .. } => (Vec::new(), Vec::new()),
            };

            let unit_name: Arc<str> = self.arena[key].name.clone();
            for dep_name in requires {
                match self
                    .name_index
                    .get(dep_name.as_str())
                    .copied()
                    .or_else(|| self.provides_index.get(dep_name.as_str()).copied())
                {
                    Some(dep_key) => queue.push_back(dep_key),
                    None if self.boot_lenient => {
                        ghosttype_log(
                            "WARN",
                            &format!(
                                "'{unit_name}' requires missing '{dep_name}' — skipping dependency"
                            ),
                        );
                    }
                    None => {
                        return Err(ForgeError::MissingUnit {
                            unit: unit_name.to_string(),
                            dependency: dep_name.clone(),
                        });
                    }
                }
            }
            for dep_name in wants {
                match self
                    .name_index
                    .get(dep_name.as_str())
                    .copied()
                    .or_else(|| self.provides_index.get(dep_name.as_str()).copied())
                {
                    Some(dep_key) => queue.push_back(dep_key),
                    None => ghosttype_log(
                        "WARN",
                        &format!("'{unit_name}' wants missing '{dep_name}' — skipping"),
                    ),
                }
            }
        }

        self.active_units = seen;
        self.expand_socket_dependencies();
        self.filter_active_by_runlevel(&target_name);
        self.required_units = self.collect_required_units();
        ghosttype_log(
            "TARGET",
            &format!(
                "Target '{}' activates {} unit(s) ({} required)",
                target_name,
                self.active_units.len(),
                self.required_units.len()
            ),
        );
        Ok(())
    }

    fn filter_active_by_runlevel(&mut self, target_name: &str) {
        if cfg!(test) || !crate::rc_update::is_database_active() {
            return;
        }
        let runlevel = crate::rc_update::target_to_runlevel(target_name);
        let before = self.active_units.len();
        self.active_units.retain(|&key| {
            if let UnitKind::Service { runlevels, .. } = &self.arena[key].kind {
                crate::rc_update::is_enabled(&self.arena[key].name, &runlevel, runlevels)
            } else {
                true
            }
        });
        let removed = before.saturating_sub(self.active_units.len());
        if removed > 0 {
            ghosttype_log(
                "RC",
                &format!(
                    "Runlevel '{runlevel}': excluded {removed} service(s) (rc-update / runlevels)"
                ),
            );
        }
    }

    /// All units reachable via hard `requires`/`need` edges within the active closure.
    fn collect_required_units(&self) -> HashSet<UnitKey> {
        let mut required = HashSet::new();
        for &key in &self.active_units {
            self.collect_requires_from(key, &mut required);
        }
        required
    }

    fn collect_requires_from(&self, key: UnitKey, seen: &mut HashSet<UnitKey>) {
        if !seen.insert(key) {
            return;
        }
        let requires = match &self.arena[key].kind {
            UnitKind::Target { requires, .. } => requires.clone(),
            UnitKind::Service { requires, .. } => requires.clone(),
            UnitKind::Socket { service, .. } => service.clone().into_iter().collect(),
            _ => Vec::new(),
        };
        for dep_name in requires {
            if let Some(dep_key) = self.name_index.get(dep_name.as_str()).copied() {
                self.collect_requires_from(dep_key, seen);
            }
        }
    }

    fn is_required(&self, key: UnitKey) -> bool {
        self.required_units.contains(&key)
    }

    fn expand_socket_dependencies(&mut self) {
        let mut changed = true;
        while changed {
            changed = false;
            let keys: Vec<UnitKey> = self.active_units.iter().copied().collect();
            for key in keys {
                if let UnitKind::Service { sockets, .. } = &self.arena[key].kind {
                    for socket_name in sockets {
                        if let Some(sock_key) = self.name_index.get(socket_name.as_str()).copied() {
                            if self.active_units.insert(sock_key) {
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn resolve_dependencies(&mut self) -> Result<(), ForgeError> {
        self.reverse_dependencies.clear();
        let keys: Vec<UnitKey> = self.active_units.iter().copied().collect();
        for key in keys {
            if !self.active_units.contains(&key) {
                continue;
            }
            // Borrow to avoid full clones when possible
            let (name_ref, after_ref, requires_ref): (&Arc<str>, &[String], &[String]) =
                match &self.arena[key].kind {
                    UnitKind::Service {
                        after, requires, ..
                    } => (&self.arena[key].name, after, requires),
                    UnitKind::Socket { after, .. } => (&self.arena[key].name, after, &[]),
                    UnitKind::Device { after, .. } => (&self.arena[key].name, after, &[]),
                    UnitKind::Timer { after, .. } => (&self.arena[key].name, after, &[]),
                    UnitKind::Mount { after, .. } => (&self.arena[key].name, after, &[]),
                    UnitKind::Target { .. } => continue,
                };

            // Build ordering without cloning input lists up front
            let mut ordering: Vec<&String> = after_ref.iter().collect();
            for dep in requires_ref {
                if !ordering.iter().any(|d| *d == dep) {
                    ordering.push(dep);
                }
            }

            let mut deps = Vec::new();
            for dep_name in ordering {
                let Some(dep_key) = self
                    .name_index
                    .get(dep_name.as_str())
                    .copied()
                    .or_else(|| self.provides_index.get(dep_name.as_str()).copied())
                else {
                    if self.boot_lenient {
                        ghosttype_log(
                            "WARN",
                            &format!("'{}' after missing '{}' — skipping", name_ref, dep_name),
                        );
                        continue;
                    }
                    return Err(ForgeError::MissingUnit {
                        unit: name_ref.to_string(),
                        dependency: dep_name.clone(),
                    });
                };
                if self.active_units.contains(&dep_key)
                    && !matches!(self.arena[dep_key].kind, UnitKind::Target { .. })
                {
                    deps.push(dep_key);
                    self.reverse_dependencies
                        .entry(dep_key)
                        .or_default()
                        .push(key);
                }
            }
            self.arena[key].dependencies = deps;
        }
        Ok(())
    }

    fn bootable_units(&self) -> Vec<UnitKey> {
        self.active_units
            .iter()
            .copied()
            .filter(|key| {
                matches!(
                    self.arena[*key].kind,
                    UnitKind::Service { .. }
                        | UnitKind::Socket { .. }
                        | UnitKind::Device { .. }
                        | UnitKind::Mount { .. }
                )
            })
            .collect()
    }

    pub fn pending_timer_jobs(&self) -> Vec<(String, Instant)> {
        let boot_base = self.boot_started;
        self.arena
            .iter()
            .filter_map(|(key, unit)| {
                if !self.active_units.contains(&key) {
                    return None;
                }
                if let UnitKind::Timer {
                    unit: target,
                    on_boot_sec,
                    fired,
                    ..
                } = &unit.kind
                {
                    if *fired {
                        return None;
                    }
                    let delay = on_boot_sec.as_ref().copied()?;
                    Some((target.clone(), boot_base + Duration::from_secs_f64(delay)))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn mark_timer_fired(&mut self, timer_unit: &str) {
        if let Some(key) = self.name_index.get(timer_unit).copied() {
            if let UnitKind::Timer { fired, .. } = &mut self.arena[key].kind {
                *fired = true;
            }
        }
    }

    pub fn timer_for_service(&self, service: &str) -> Option<String> {
        self.arena.iter().find_map(|(_, unit)| {
            if let UnitKind::Timer { unit: target, .. } = &unit.kind {
                if target == service {
                    return Some(unit.name.to_string());
                }
            }
            None
        })
    }

    pub fn device_units(&self) -> Vec<(String, String)> {
        self.arena
            .iter()
            .filter_map(|(_, unit)| {
                if let UnitKind::Device { path, service, .. } = &unit.kind {
                    let kernel = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    Some((kernel, service.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn pending_device_units(&self) -> Vec<(UnitKey, PathBuf, String)> {
        self.arena
            .iter()
            .filter_map(|(key, unit)| {
                if let UnitKind::Device {
                    path,
                    service,
                    activated,
                    ..
                } = &unit.kind
                {
                    if !*activated {
                        return Some((key, path.clone(), service.clone()));
                    }
                }
                None
            })
            .collect()
    }

    pub fn activate_device_by_key(&mut self, key: UnitKey) -> Result<(), ForgeError> {
        let service = match &mut self.arena[key].kind {
            UnitKind::Device {
                activated, service, ..
            } => {
                if *activated {
                    return Ok(());
                }
                *activated = true;
                service.clone()
            }
            _ => return Ok(()),
        };
        ghosttype_log("DEVICE", &format!("Device ready — activating '{service}'"));
        self.start_service_by_name(&service).map(|_| ())
    }

    pub fn activate_ready_devices(&mut self) -> Result<(), ForgeError> {
        let ready: Vec<(UnitKey, String)> = self
            .pending_device_units()
            .into_iter()
            .filter(|(_, path, _)| path.exists())
            .map(|(key, _, service)| (key, service))
            .collect();

        for (key, _service) in ready {
            self.activate_device_by_key(key)?;
        }
        Ok(())
    }

    pub fn boot_waves(&self) -> Result<Vec<Vec<UnitKey>>, ForgeError> {
        let bootable: HashSet<UnitKey> = self.bootable_units().into_iter().collect();
        let mut in_degree: HashMap<UnitKey, usize> = bootable
            .iter()
            .map(|key| (*key, self.arena[*key].dependencies.len()))
            .collect();

        let mut ready: VecDeque<UnitKey> = in_degree
            .iter()
            .filter_map(|(key, degree)| (*degree == 0).then_some(*key))
            .collect();

        let mut waves = Vec::new();
        let mut started = 0usize;

        while !ready.is_empty() {
            let mut wave: Vec<UnitKey> = ready.drain(..).collect();
            // Avoid expensive owned clones for sort key
            wave.sort_by(|a, b| self.arena[*a].name.cmp(&self.arena[*b].name));
            started += wave.len();

            for key in &wave {
                if let Some(dependents) = self.reverse_dependencies.get(key) {
                    for &other_key in dependents {
                        if bootable.contains(&other_key) {
                            if let Some(degree) = in_degree.get_mut(&other_key) {
                                *degree -= 1;
                                if *degree == 0 {
                                    ready.push_back(other_key);
                                }
                            }
                        }
                    }
                }
            }
            waves.push(wave);
        }

        if started != bootable.len() {
            return Err(ForgeError::CycleDetected);
        }
        Ok(waves)
    }

    pub fn boot_with_fallback(&mut self, extra_fallbacks: &[&str]) -> bool {
        let mut chain = vec![self.active_target.clone()];
        for target in extra_fallbacks {
            let name = (*target).to_string();
            if !chain.contains(&name) {
                chain.push(name);
            }
        }

        for target in chain {
            self.active_target = target.clone();
            ghosttype_log("BOOT", &format!("Attempting target '{target}'"));
            match self.boot_parallel() {
                Ok(()) => {
                    ghosttype_log("BOOT", &format!("Target '{target}' reached"));
                    return true;
                }
                Err(err) => ghosttype_log(
                    "WARN",
                    &format!("Target '{target}' failed: {err} — trying fallback"),
                ),
            }
        }
        false
    }

    pub fn boot_parallel(&mut self) -> Result<(), ForgeError> {
        self.resolve_target_closure()?;
        self.resolve_dependencies()?;

        let _ = cgroup::prepare_hierarchy();

        // NOTE: The caller (main) holds the &mut lock for the entire boot.
        // This includes blocking waits in start_service (notify, forking, oneshot, dbus).
        // Future: split into prepare + fire (non-blocking spawns) + readiness phase
        // outside the main boot lock or using the reactor for event-driven completion.
        // See subagent findings and plan for lock-hold reduction.

        // Activate socket units first (within wave order).
        let waves = match self.boot_waves() {
            Ok(w) => w,
            Err(ForgeError::CycleDetected) if self.boot_lenient => {
                ghosttype_log(
                    "WARN",
                    "Dependency cycle detected — booting reachable units only",
                );
                vec![self.bootable_units()]
            }
            Err(err) => return Err(err),
        };
        ghosttype_log("DAG", &format!("Scheduling {} boot wave(s)", waves.len()));

        for (index, wave) in waves.iter().enumerate() {
            let wave_start = Instant::now();
            ghosttype_log(
                "WAVE",
                &format!("Wave {} — {} unit(s)", index + 1, wave.len()),
            );

            for key in wave {
                if matches!(self.arena[*key].kind, UnitKind::Socket { .. }) {
                    if let Err(e) = self.activate_socket_unit(*key) {
                        ghosttype_log("FAILED", &format!("socket activation: {e}"));
                        self.arena[*key].state = UnitState::Failed;
                        if self.strict_boot && self.is_required(*key) {
                            return Err(e);
                        }
                    }
                }
            }
            for key in wave {
                if matches!(self.arena[*key].kind, UnitKind::Mount { .. }) {
                    if let Err(e) = self.activate_mount_unit(*key) {
                        ghosttype_log("FAILED", &format!("mount activation: {e}"));
                        self.arena[*key].state = UnitState::Failed;
                        if self.strict_boot && self.is_required(*key) {
                            return Err(e);
                        }
                    }
                }
            }
            for key in wave {
                if matches!(self.arena[*key].kind, UnitKind::Service { .. }) {
                    if let Err(e) = self.start_service(*key) {
                        ghosttype_log("FAILED", &format!("service start: {e}"));
                        if let Some(u) = self.arena.get_mut(*key) {
                            u.state = UnitState::Failed;
                        }
                        if self.strict_boot && self.is_required(*key) {
                            return Err(e);
                        }
                    }
                }
            }

            // NOTE for intra-wave parallelism (subagent + plan):
            // Services within a wave have no hard deps between them.
            // Currently serial. Future: use std::thread::scope to concurrently
            // spawn independent services (the spawn + pre_exec part is the fast part;
            // readiness waits can be joined or driven by reactor events).
            // Care needed because start_service takes &mut self and does blocking I/O.

            let names: Vec<String> = wave
                .iter()
                .map(|k| self.arena[*k].name.to_string())
                .collect();
            self.boot_profile.waves.push(WaveTiming {
                wave: index + 1,
                services: names,
                duration_ms: wave_start.elapsed().as_millis(),
            });
            self.reap_all_pending();
        }

        self.boot_profile.total_boot_ms = self.boot_started.elapsed().as_millis();
        ghosttype_log(
            "PROFILE",
            &format!(
                "Boot complete in {} ms (target: {})",
                self.boot_profile.total_boot_ms, self.active_target
            ),
        );
        let _ = self.activate_ready_devices();
        Ok(())
    }

    fn activate_socket_unit(&mut self, key: UnitKey) -> Result<(), ForgeError> {
        let name = self.arena[key].name.to_string();
        let listen = match &self.arena[key].kind {
            UnitKind::Socket { listen, .. } => listen.clone(),
            _ => return Ok(()),
        };

        ghosttype_log("SOCKET", &format!("Activating socket unit '{}'", name));
        self.arena[key].state = UnitState::Starting;

        let activation =
            activate_sockets(&name, &listen).map_err(|e| ForgeError::SocketFailed {
                name: name.to_string(),
                error: e,
            })?;

        if let UnitKind::Socket {
            activation: slot, ..
        } = &mut self.arena[key].kind
        {
            *slot = Some(activation);
        }
        self.arena[key].state = UnitState::Listening;
        ghosttype_log("SOCKET", &format!("Socket '{}' listening", name));
        Ok(())
    }

    fn activate_mount_unit(&mut self, key: UnitKey) -> Result<(), ForgeError> {
        let (name, what, where_, fstype, options) = {
            let unit = &mut self.arena[key];
            match &mut unit.kind {
                UnitKind::Mount {
                    what,
                    where_,
                    fstype,
                    options,
                    mounted,
                    ..
                } => {
                    if *mounted {
                        return Ok(());
                    }
                    unit.state = UnitState::Starting;
                    (
                        unit.name.to_string(),
                        what.clone(),
                        where_.clone(),
                        fstype.clone(),
                        options.clone(),
                    )
                }
                _ => return Ok(()),
            }
        };

        ghosttype_log(
            "MOUNT",
            &format!("Activating mount unit '{name}' ({where_})"),
        );
        if let Err(e) = crate::mount_unit::mount_filesystem(&what, &where_, &fstype, &options) {
            if std::process::id() == 1 {
                return Err(ForgeError::InvalidOperation(format!("mount '{name}': {e}")));
            }
            ghosttype_log("WARN", &format!("Mount '{name}' skipped in sandbox: {e}"));
            self.arena[key].state = UnitState::Dead;
            return Ok(());
        }

        if let UnitKind::Mount { mounted, .. } = &mut self.arena[key].kind {
            *mounted = true;
        }
        self.arena[key].state = UnitState::Running;
        crate::journal::record(&name, 6, format!("Mounted {what} on {where_}"), None);
        Ok(())
    }

    pub fn spawn_adhoc(
        &mut self,
        name: &str,
        binary: &str,
        args: Vec<String>,
    ) -> Result<u32, ForgeError> {
        let key = self.register_service(
            name.to_string(),
            binary.to_string(),
            args,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RestartPolicy::No,
            ServiceLimits::default(),
        )?;
        self.active_units.insert(key);
        self.start_service(key)
    }

    pub fn start_service_by_name(&mut self, name: &str) -> Result<u32, ForgeError> {
        let key = self
            .name_index
            .get(name)
            .copied()
            .or_else(|| self.provides_index.get(name).copied())
            .ok_or_else(|| ForgeError::NotFound(name.to_string()))?;
        if !matches!(self.arena[key].kind, UnitKind::Service { .. }) {
            return Err(ForgeError::NotService(name.to_string()));
        }
        self.active_units.insert(key);
        self.start_service(key)
    }

    pub fn stop_service_by_name(&mut self, name: &str) -> Result<(), ForgeError> {
        let key = self
            .name_index
            .get(name)
            .copied()
            .or_else(|| self.provides_index.get(name).copied())
            .ok_or_else(|| ForgeError::NotFound(name.to_string()))?;
        self.stop_service(key, Signal::SIGTERM)
    }

    pub fn restart_service_by_name(&mut self, name: &str) -> Result<u32, ForgeError> {
        let key = self
            .name_index
            .get(name)
            .copied()
            .or_else(|| self.provides_index.get(name).copied())
            .ok_or_else(|| ForgeError::NotFound(name.to_string()))?;

        if let UnitKind::Service { pid: Some(pid), .. } = self.arena[key].kind {
            let _ = self.stop_service(key, Signal::SIGTERM);
            for _ in 0..20 {
                self.reap_all_pending();
                if !self.pid_index.contains_key(&pid) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        self.start_service(key)
    }

    /// Send reload signal / run reload command without stopping the service.
    /// Resolves virtual provider names so `forgectl reload net` works.
    pub fn reload_service_by_name(&mut self, name: &str) -> Result<(), ForgeError> {
        let key = self
            .name_index
            .get(name)
            .copied()
            .or_else(|| self.provides_index.get(name).copied())
            .ok_or_else(|| ForgeError::NotFound(name.to_string()))?;

        if !matches!(self.arena[key].kind, UnitKind::Service { .. }) {
            return Err(ForgeError::NotService(name.to_string()));
        }

        let (pid, reload_cmd, reload_signal_str) = match &self.arena[key].kind {
            UnitKind::Service {
                pid: Some(pid),
                reload_cmd,
                reload_signal,
                ..
            } => (*pid, reload_cmd.clone(), reload_signal.clone()),
            UnitKind::Service { pid: None, .. } => {
                return Err(ForgeError::NotRunning(self.arena[key].name.to_string()));
            }
            _ => unreachable!(),
        };

        let unit_name = self.arena[key].name.clone();
        ghosttype_log("RELOAD", &format!("Reloading '{unit_name}'"));

        if let Some((cmd, args)) = reload_cmd {
            let status =
                Command::new(&cmd)
                    .args(&args)
                    .status()
                    .map_err(|e| ForgeError::SpawnFailed {
                        name: unit_name.to_string(),
                        error: format!("reload command: {e}"),
                    })?;
            if !status.success() {
                return Err(ForgeError::SpawnFailed {
                    name: unit_name.to_string(),
                    error: format!("reload command exited {status}"),
                });
            }
        } else {
            let signal = reload_signal_str
                .as_deref()
                .map(parse_signal)
                .unwrap_or(Signal::SIGHUP);
            nix::sys::signal::kill(Pid::from_raw(pid as i32), signal).map_err(|e| {
                ForgeError::SpawnFailed {
                    name: unit_name.to_string(),
                    error: format!("kill({signal:?}): {e}"),
                }
            })?;
        }

        crate::journal::record(&unit_name, 6, "Service reloaded".to_string(), Some(pid));
        ghosttype_log("RELOAD", &format!("'{unit_name}' reloaded successfully"));
        Ok(())
    }

    fn service_timeout(&self, service_type: ServiceType, timeout_secs: Option<u64>) -> Duration {
        if let Some(secs) = timeout_secs {
            return Duration::from_secs(secs);
        }
        match service_type {
            ServiceType::Oneshot => oneshot_timeout(),
            _ => Duration::from_secs(90),
        }
    }

    fn write_pidfile(path: &str, pid: u32) -> Result<(), String> {
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        fs::write(path, format!("{pid}\n")).map_err(|e| e.to_string())
    }

    fn start_service(&mut self, key: UnitKey) -> Result<u32, ForgeError> {
        let (
            name,
            exec,
            args,
            socket_names,
            cgroup_limits,
            service_type,
            environment,
            exec_start_pre,
            bus_name,
            sandbox_cfg,
            user,
            group,
            working_directory,
            timeout_secs,
            pidfile,
        ) = {
            let unit = &mut self.arena[key];
            match &mut unit.kind {
                UnitKind::Service {
                    exec,
                    args,
                    sockets,
                    cgroup,
                    service_type,
                    environment,
                    exec_start_pre,
                    bus_name,
                    sandbox,
                    user,
                    group,
                    working_directory,
                    timeout_secs,
                    pidfile,
                    pid: _,
                    ..
                } => {
                    unit.state = UnitState::Starting;
                    (
                        unit.name.to_string(),
                        exec.clone(),
                        args.clone(),
                        sockets.clone(),
                        cgroup.clone(),
                        *service_type,
                        environment.clone(),
                        exec_start_pre.clone(),
                        bus_name.clone(),
                        sandbox.clone(),
                        user.clone(),
                        group.clone(),
                        working_directory.clone(),
                        *timeout_secs,
                        pidfile.clone(),
                    )
                }
                _ => {
                    return Err(ForgeError::NotService(unit.name.to_string()));
                }
            }
        };

        if &*name == "forge-early" {
            ghosttype_log("LAUNCHING", "Starting 'forge-early' (Rust early-boot)");
            crate::early_boot::run().map_err(|e| ForgeError::SpawnFailed {
                name: name.to_string(),
                error: e,
            })?;
            self.arena[key].state = UnitState::Dead;
            return Ok(0);
        }

        let startup_timeout = self.service_timeout(service_type, timeout_secs);

        ghosttype_log("LAUNCHING", &format!("Starting '{}' via {}", name, exec));
        crate::journal::record(&name, 6, format!("Starting service via {exec}"), None);

        if &*name == "user-sessions" {
            let _ = crate::user_sessions::allow_logins();
        }

        if std::process::id() != 1 {
            if let Some(reason) = Self::sandbox_skip_reason(&name) {
                return self.adopt_host_service(key, &format!("sandbox skip — {reason}"));
            }
            if &*name == "dbus" && Path::new("/run/dbus/system_bus_socket").exists() {
                return self.adopt_host_service(
                    key,
                    "using host system D-Bus (not PID 1 — skip starting dbus-daemon)",
                );
            }
            if &*name == "NetworkManager" && Self::host_process_running("NetworkManager") {
                return self.adopt_host_service(
                    key,
                    "using host NetworkManager (not PID 1 — skip second instance)",
                );
            }
            if &*name == "udev"
                && (Self::host_process_running("systemd-udevd")
                    || Self::host_process_running("udevd"))
            {
                return self.adopt_host_service(
                    key,
                    "using host udevd (not PID 1 — skip second instance)",
                );
            }
            if &*name == "logind"
                && (Self::host_process_running("systemd-logind")
                    || Self::host_process_running("elogind"))
            {
                return self.adopt_host_service(
                    key,
                    "using host logind (elogind or systemd-logind) (not PID 1 — skip second instance)",
                );
            }
            if &*name == "polkit" && Self::host_process_running("polkitd") {
                return self.adopt_host_service(
                    key,
                    "using host polkitd (not PID 1 — skip second instance)",
                );
            }
            if &*name == "accounts-daemon" && Self::host_process_running("accounts-daemon") {
                return self.adopt_host_service(
                    key,
                    "using host accounts-daemon (not PID 1 — skip second instance)",
                );
            }
        }

        if let Some((pre_exec, pre_args)) = exec_start_pre {
            let status = Command::new(&pre_exec)
                .args(&pre_args)
                .status()
                .map_err(|e| ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: format!("ExecStartPre failed: {e}"),
                })?;
            if !status.success() {
                return Err(ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: format!("ExecStartPre exited with {status}"),
                });
            }
        }

        let listen_fds = self.collect_socket_fds(&socket_names)?;
        let (log_file, log_path) = self.open_service_log(&name)?;
        if std::process::id() == 1 && &*name == "dbus" {
            let _ = std::fs::write(&log_path, b"");
        }

        let mut cmd =
            if let Some(resolved) = crate::launch::resolve_service_command(&name, &exec, &args) {
                resolved.map_err(|e| ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: e,
                })?
            } else if std::process::id() == 1 && &*name == "dbus" {
                crate::boot_debug::log(&format!("dbus: Rust launcher (exec={exec})"));
                let using_activation = !listen_fds.is_empty();
                if using_activation {
                    crate::boot_debug::log(&format!(
                        "dbus: LISTEN_FDS={} for socket activation",
                        listen_fds.len()
                    ));
                    let _ = std::fs::create_dir_all("/run/dbus");
                    let _ = std::fs::create_dir_all("/var/lib/dbus");
                    if Path::new("/usr/bin/dbus-uuidgen").is_file() {
                        let _ = Command::new("/usr/bin/dbus-uuidgen")
                            .arg("--ensure=/var/lib/dbus/machine-id")
                            .status();
                    }
                } else {
                    crate::dbus_launch::prepare_runtime();
                }
                crate::dbus_launch::system_dbus_command().map_err(|e| ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: e,
                })?
            } else {
                let mut c = Command::new(&exec);
                c.args(&args);
                c
            };

        // Apply standard systemd service settings (mirroring systemd exec context)
        if let Some(ref wd) = working_directory {
            if !wd.is_empty() {
                cmd.current_dir(wd);
            }
        }
        if let Some(ref u) = user {
            if let Some(uid) = resolve_user_to_uid(u) {
                cmd.uid(uid);
            } else {
                ghosttype_log(
                    "WARN",
                    &format!("Could not resolve User={} for {}", u, name),
                );
            }
        }
        if let Some(ref g) = group {
            if let Some(gid) = resolve_group_to_gid(g) {
                cmd.gid(gid);
            }
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::from(log_file.try_clone().map_err(|e| {
                ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: e.to_string(),
                }
            })?))
            .stderr(Stdio::from(log_file));

        for (k, v) in merge_environment(&[], &environment) {
            cmd.env(k, v);
        }

        let wants_notify = matches!(
            service_type,
            ServiceType::Notify | ServiceType::NotifyReload
        );
        let notify_sock = if wants_notify {
            Some(
                prepare_notify_socket(&name).map_err(|e| ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: e,
                })?,
            )
        } else {
            None
        };

        if let Some((path, _sock)) = &notify_sock {
            cmd.env("NOTIFY_SOCKET", path);
        }

        let has_listen = !listen_fds.is_empty();
        let listen_count = listen_fds.len();
        if has_listen {
            cmd.env("LISTEN_FDS", listen_count.to_string());
            cmd.env(
                "FORGE_SOCKET_PATHS",
                listen_fds
                    .iter()
                    .map(|(_, path)| path.as_str())
                    .collect::<Vec<_>>()
                    .join(":"),
            );
            // LISTEN_PID must be the child's own PID (set inside pre_exec after fork).
        }

        // Wrapper scripts (dbus, polkit) handle SELinux/user drop themselves as root.
        let drop_user: Option<String> = None;
        if let Some(ref user) = drop_user {
            ghosttype_log(
                "PRIV",
                &format!("'{name}' will drop privileges to user '{user}'"),
            );
        }

        // Keep socket FDs open and inheritable for the child (systemd-style fd passing).
        // FDs will be renumbered to 3,4,... (SD_LISTEN_FDS_START) and LISTEN_PID set
        // inside the child so that sd_listen_fds() / dbus-broker see standard fds.
        let inherited: Vec<OwnedFd> = listen_fds.into_iter().map(|(fd, _)| fd).collect();
        let sandbox_child = sandbox_cfg.clone();
        let service_name = name.clone();

        let fds_len = inherited.len();
        let fds_val = std::ffi::CString::new(fds_len.to_string()).unwrap();

        // Wrap in Option so the FnMut closure can take ownership on first (only) invocation.
        let mut inherited_opt: Option<Vec<OwnedFd>> = if inherited.is_empty() {
            None
        } else {
            Some(inherited)
        };
        unsafe {
            cmd.pre_exec(move || {
                if let Some(ref user) = drop_user {
                    crate::privdrop::drop_to_user(user).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::PermissionDenied, e)
                    })?;
                }
                crate::sandbox::apply_in_child(&sandbox_child, &service_name)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;

                if let Some(inherited) = inherited_opt.take() {
                    let n = inherited.len() as libc::c_int;

                    // Set LISTEN_PID using a fork-safe format.
                    let mut pid_buf = [0u8; 16];
                    let pid = libc::getpid();
                    let mut val = pid;
                    let mut idx = 15;
                    pid_buf[idx] = 0; // null terminator
                    if val == 0 {
                        idx -= 1;
                        pid_buf[idx] = b'0';
                    } else {
                        while val > 0 && idx > 0 {
                            idx -= 1;
                            pid_buf[idx] = b'0' + (val % 10) as u8;
                            val /= 10;
                        }
                    }
                    let name_pid = b"LISTEN_PID\0";
                    libc::setenv(
                        name_pid.as_ptr() as *const libc::c_char,
                        pid_buf[idx..].as_ptr() as *const libc::c_char,
                        1,
                    );

                    let name_fds = b"LISTEN_FDS\0";
                    libc::setenv(
                        name_fds.as_ptr() as *const libc::c_char,
                        fds_val.as_ptr(),
                        1,
                    );

                    // First, safely move any source fds that overlap with the target range 3..3+n.
                    let mut temp_fds = Vec::with_capacity(inherited.len());
                    for owned in inherited {
                        let src = owned.as_raw_fd();
                        let safe_fd = if src >= 3 && src < 3 + n {
                            let new_fd = libc::fcntl(src, libc::F_DUPFD_CLOEXEC, 3 + n);
                            if new_fd == -1 {
                                return Err(std::io::Error::last_os_error());
                            }
                            let _ = libc::close(src);
                            new_fd
                        } else {
                            src
                        };
                        temp_fds.push(safe_fd);
                        ::std::mem::forget(owned);
                    }

                    // Now duplicate to target fds 3, 4, ...
                    let mut target: libc::c_int = 3;
                    for src in temp_fds {
                        if libc::dup2(src, target) == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        // Ensure the target fd has no close-on-exec.
                        let _ = libc::fcntl(target, libc::F_SETFD, 0);
                        if src != target {
                            let _ = libc::close(src);
                        }
                        target += 1;
                    }
                }

                Ok(())
            });
        }

        let child = cmd.spawn().map_err(|e| ForgeError::SpawnFailed {
            name: name.to_string(),
            error: e.to_string(),
        })?;

        let pid = child.id();
        if let UnitKind::Service { pid: slot, .. } = &mut self.arena[key].kind {
            *slot = Some(pid);
        }

        let check_activation = || -> Result<(), ForgeError> {
            if wants_notify {
                if let Some((_, sock)) = notify_sock {
                    let ready = wait_for_ready(&sock, startup_timeout).map_err(|e| {
                        ForgeError::SpawnFailed {
                            name: name.to_string(),
                            error: e,
                        }
                    })?;
                    if !ready {
                        return Err(ForgeError::SpawnFailed {
                            name: name.to_string(),
                            error: "notify service did not send READY=1".into(),
                        });
                    }
                    ghosttype_log("NOTIFY", &format!("'{}' sent READY=1", name));

                    // Quick non-blocking drain to catch any immediate extra messages (WATCHDOG, STATUS, etc.)
                    let _ = sock.set_read_timeout(Some(Duration::from_millis(0)));
                    let mut drain_buf = [0u8; 256];
                    for _ in 0..4 {
                        match sock.recv(&mut drain_buf) {
                            Ok(n) => {
                                let msg = String::from_utf8_lossy(&drain_buf[..n]);
                                let parsed = parse_notify(&msg);
                                // We don't have easy key here for update_watchdog yet; check_watchdogs will use time-based.
                                if parsed.watchdog || parsed.watchdog_usec.is_some() {
                                    ghosttype_log(
                                        "WATCHDOG",
                                        &format!("early watchdog info for '{}'", name),
                                    );
                                }
                            }
                            _ => break,
                        }
                    }
                }
            }

            // Record watchdog info if provided in initial notifies (or side channel).
            // Full ongoing monitoring added via check_watchdogs.
            if service_type == ServiceType::Notify || service_type == ServiceType::NotifyReload {
                // Re-check last received if we can drain quickly; the wait_for_ready may have seen some.
                // For simplicity, services typically set WATCHDOG_USEC via env or first message.
                // If present on unit from unit file we could have it; here we just leave hook.
            }

            if service_type == ServiceType::Dbus {
                let bus = bus_name.clone().ok_or_else(|| ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: "Type=dbus requires bus_name".into(),
                })?;
                if &*name == "dbus" {
                    // Special case for the system bus provider: do not do a long client query
                    // against a bus that is still initializing. Give it a moment to take the fd
                    // and start accepting, then proceed. Dependent units poll for the bus anyway.
                    std::thread::sleep(Duration::from_millis(400));
                    crate::boot_debug::log(
                        "dbus: core bus launched, skipping self-wait (dependents will poll)",
                    );
                    // optionally do a quick probe
                    if Path::new("/run/dbus/system_bus_socket").exists() {
                        if let Ok(_) =
                            std::os::unix::net::UnixStream::connect("/run/dbus/system_bus_socket")
                        {
                            ghosttype_log("DBUS", "'dbus' core bus socket responsive");
                        }
                    }
                } else {
                    let acquired =
                        crate::dbus_wait::wait_for_bus_name(&bus, startup_timeout, Some(pid))
                            .map_err(|e| ForgeError::SpawnFailed {
                                name: name.to_string(),
                                error: e,
                            })?;
                    if !acquired {
                        return Err(ForgeError::SpawnFailed {
                            name: name.to_string(),
                            error: format!("D-Bus name '{bus}' not acquired within timeout"),
                        });
                    }
                    ghosttype_log("DBUS", &format!("'{}' owns bus name '{bus}'", name));
                }
            }
            Ok(())
        };

        if let Err(err) = check_activation() {
            ghosttype_log(
                "WARN",
                &format!("Service '{name}' failed activation, terminating PID {pid}"),
            );
            let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
            if let UnitKind::Service { pid: slot, .. } = &mut self.arena[key].kind {
                *slot = None;
            }
            let mut status = 0;
            let _ = unsafe { libc::waitpid(pid as i32, &mut status, 0) };
            return Err(err);
        }

        if let Some(ref path) = pidfile {
            if let Err(e) = Self::write_pidfile(path, pid) {
                ghosttype_log("WARN", &format!("pidfile '{path}' for '{name}': {e}"));
            }
        }

        if service_type == ServiceType::Forking {
            let status = match self.wait_for_service_exit(key, &name, pid, Duration::from_secs(30))
            {
                Ok(s) => s,
                Err(err) => {
                    let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                    if let UnitKind::Service { pid: slot, .. } = &mut self.arena[key].kind {
                        *slot = None;
                    }
                    let mut status = 0;
                    let _ = unsafe { libc::waitpid(pid as i32, &mut status, 0) };
                    return Err(err);
                }
            };
            let success = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0;
            if !success {
                let detail = if libc::WIFEXITED(status) {
                    format!("exit code {}", libc::WEXITSTATUS(status))
                } else if libc::WIFSIGNALED(status) {
                    format!("signal {}", libc::WTERMSIG(status))
                } else {
                    "abnormal exit".into()
                };
                if let UnitKind::Service { pid: slot, .. } = &mut self.arena[key].kind {
                    *slot = None;
                }
                return Err(ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: format!("forking parent failed ({detail})"),
                });
            }
            let daemon_pid = match self.find_forking_daemon(&exec, &name) {
                Ok(d) => d,
                Err(err) => {
                    if let UnitKind::Service { pid: slot, .. } = &mut self.arena[key].kind {
                        *slot = None;
                    }
                    return Err(err);
                }
            };
            if let UnitKind::Service { pid: slot, .. } = &mut self.arena[key].kind {
                *slot = Some(daemon_pid);
            }
            self.arena[key].state = UnitState::Running;
            self.pid_index.insert(daemon_pid, key);
            let _ = cgroup::attach_service(&name, daemon_pid, &cgroup_limits);
            ghosttype_log(
                "ONLINE",
                &format!(
                    "'{name}' forking daemon PID {daemon_pid} (log: {})",
                    log_path.display()
                ),
            );
            crate::journal::record(
                &name,
                6,
                format!("Forking service online (pid {daemon_pid})"),
                Some(daemon_pid),
            );
            return Ok(daemon_pid);
        }

        if service_type == ServiceType::Oneshot {
            let status = match self.wait_for_service_exit(key, &name, pid, startup_timeout) {
                Ok(s) => s,
                Err(err) => {
                    let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                    if let UnitKind::Service { pid: slot, .. } = &mut self.arena[key].kind {
                        *slot = None;
                    }
                    let mut status = 0;
                    let _ = unsafe { libc::waitpid(pid as i32, &mut status, 0) };
                    return Err(err);
                }
            };
            let success = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0;
            if let UnitKind::Service { pid: slot, .. } = &mut self.arena[key].kind {
                *slot = None;
            }
            self.arena[key].state = if success {
                UnitState::Dead
            } else {
                UnitState::Failed
            };
            if !success {
                let detail = if libc::WIFEXITED(status) {
                    format!("exit code {}", libc::WEXITSTATUS(status))
                } else if libc::WIFSIGNALED(status) {
                    format!("signal {}", libc::WTERMSIG(status))
                } else {
                    "abnormal exit".into()
                };
                return Err(ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: format!("oneshot failed ({detail})"),
                });
            }
            ghosttype_log("ONESHOT", &format!("'{name}' completed successfully"));
            crate::journal::record(&name, 6, "Oneshot service finished", None);
            return Ok(pid);
        }

        self.arena[key].state = UnitState::Running;
        self.pid_index.insert(pid, key);
        let _ = cgroup::attach_service(&name, pid, &cgroup_limits);

        ghosttype_log(
            "ONLINE",
            &format!(
                "'{}' bound to PID {} (log: {})",
                name,
                pid,
                log_path.display()
            ),
        );
        crate::journal::record(&name, 6, format!("Service online (pid {pid})"), Some(pid));
        Ok(pid)
    }

    fn find_forking_daemon(&self, exec: &str, name: &str) -> Result<u32, ForgeError> {
        let basename = Path::new(exec)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(exec);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(output) = Command::new("pgrep")
                .arg("-n")
                .arg("-x")
                .arg(basename)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
            {
                if output.status.success() {
                    let pid_str = String::from_utf8_lossy(&output.stdout);
                    if let Ok(pid) = pid_str.trim().parse::<u32>() {
                        return Ok(pid);
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: format!(
                        "forking service '{name}' did not leave a running '{basename}' process"
                    ),
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn process_pending_restarts(&mut self) {
        self.check_watchdogs();
        while let Some(key) = self.pending_restarts.pop_front() {
            let name = self.arena[key].name.clone();
            if !self.restart_allowed(&name) {
                continue;
            }
            ghosttype_log("RESTART", &format!("Restarting '{name}'"));
            if let Err(e) = self.start_service(key) {
                ghosttype_log("FAILED", &format!("Restart of '{name}' failed: {e}"));
            }
        }
    }

    fn wait_for_service_exit(
        &mut self,
        _key: UnitKey,
        name: &str,
        pid: u32,
        timeout: Duration,
    ) -> Result<i32, ForgeError> {
        let deadline = Instant::now() + timeout;
        let mut status: libc::c_int = 0;

        loop {
            let result = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
            if result == pid as i32 {
                return Ok(status);
            }
            if result < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::ECHILD) {
                    return Err(ForgeError::SpawnFailed {
                        name: name.to_string(),
                        error: "oneshot child vanished before exit status was collected".into(),
                    });
                }
                return Err(ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: format!("waitpid({name}): {err}"),
                });
            }

            self.reap_all_pending();

            if Instant::now() >= deadline {
                ghosttype_log(
                    "TIMEOUT",
                    &format!(
                        "Oneshot '{name}' (PID {pid}) exceeded {}s — sending SIGTERM",
                        timeout.as_secs()
                    ),
                );
                let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                for _ in 0..20 {
                    let result = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
                    if result == pid as i32 {
                        return Ok(status);
                    }
                    self.reap_all_pending();
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                let result = unsafe { libc::waitpid(pid as i32, &mut status, 0) };
                if result == pid as i32 {
                    return Ok(status);
                }
                return Err(ForgeError::SpawnFailed {
                    name: name.to_string(),
                    error: format!("oneshot timed out after {}s", timeout.as_secs()),
                });
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn collect_socket_fds(
        &mut self,
        socket_names: &[String],
    ) -> Result<Vec<(OwnedFd, String)>, ForgeError> {
        let mut fds = Vec::new();
        for socket_name in socket_names {
            let key = *self.name_index.get(socket_name.as_str()).ok_or_else(|| {
                ForgeError::MissingUnit {
                    unit: "service".into(),
                    dependency: socket_name.clone(),
                }
            })?;
            if !matches!(self.arena[key].kind, UnitKind::Socket { .. }) {
                return Err(ForgeError::InvalidOperation(format!(
                    "'{socket_name}' is not a socket unit"
                )));
            }
            if self.arena[key].state != UnitState::Listening {
                self.activate_socket_unit(key)?;
            }
            if let UnitKind::Socket {
                activation: Some(act),
                ..
            } = &self.arena[key].kind
            {
                for entry in &act.entries {
                    fds.push((
                        unsafe { OwnedFd::from_raw_fd(libc::dup(entry.fd.as_raw_fd())) },
                        entry.path.clone(),
                    ));
                }
            }
        }
        Ok(fds)
    }

    fn stop_service(&mut self, key: UnitKey, default_signal: Signal) -> Result<(), ForgeError> {
        let (pid, stop_cmd, stop_signal_str) = match &self.arena[key].kind {
            UnitKind::Service {
                pid: Some(pid),
                stop_cmd,
                stop_signal,
                ..
            } => (*pid, stop_cmd.clone(), stop_signal.clone()),
            _ => return Err(ForgeError::NotRunning(self.arena[key].name.to_string())),
        };

        self.arena[key].state = UnitState::Stopping;
        let name = self.arena[key].name.clone();

        if let Some((cmd, args)) = stop_cmd {
            ghosttype_log("STOP", &format!("Running stop command for '{name}': {cmd}"));
            let _ = Command::new(&cmd).args(&args).status();
        } else {
            let signal = stop_signal_str
                .as_deref()
                .map(parse_signal)
                .unwrap_or(default_signal);
            ghosttype_log(
                "STOP",
                &format!("Sending {signal:?} to '{name}' (PID {pid})"),
            );
            if let Err(e) = nix::sys::signal::kill(Pid::from_raw(pid as i32), signal) {
                ghosttype_log("WARN", &format!("kill({name}): {e}"));
            }
        }
        Ok(())
    }

    pub fn activate_target(&mut self, name: &str) -> Result<(), ForgeError> {
        let key = *self
            .name_index
            .get(name)
            .ok_or_else(|| ForgeError::NotFound(name.to_string()))?;
        if !matches!(self.arena[key].kind, UnitKind::Target { .. }) {
            return Err(ForgeError::InvalidOperation(format!(
                "'{name}' is not a target unit"
            )));
        }

        ghosttype_log("TARGET", &format!("Switching active target to '{name}'"));

        if let Ok(order) = self.stop_order() {
            for stop_key in order {
                if matches!(self.arena[stop_key].kind, UnitKind::Service { .. }) {
                    let _ = self.stop_service(stop_key, Signal::SIGTERM);
                }
            }
        }
        self.wait_for_services_stopped(Self::stop_timeout());

        self.active_target = name.to_string();
        self.boot_profile.active_target = name.to_string();
        self.persist_softlevel(name);
        self.boot_parallel()
    }

    fn wait_for_services_stopped(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            self.reap_all_pending();
            let running = self
                .bootable_units()
                .into_iter()
                .any(|key| matches!(self.arena[key].kind, UnitKind::Service { pid: Some(_), .. }));
            if !running || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        for key in self.bootable_units() {
            if let UnitKind::Service { pid: Some(pid), .. } = self.arena[key].kind {
                let name = self.arena[key].name.clone();
                ghosttype_log(
                    "KILL",
                    &format!("Escalating SIGKILL to '{}' (PID {})", name, pid),
                );
                let _ = nix::sys::signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
            }
        }
        self.reap_all_pending();
    }

    pub fn shutdown(&mut self) -> Result<(), ForgeError> {
        if self.shutting_down {
            return Ok(());
        }
        self.shutting_down = true;
        ghosttype_log(
            "SHUTDOWN",
            "Beginning graceful shutdown (reverse dependency order)",
        );

        let order = self.stop_order().unwrap_or_else(|_| {
            self.bootable_units()
                .into_iter()
                .filter(|key| matches!(self.arena[*key].kind, UnitKind::Service { .. }))
                .collect()
        });
        for key in order {
            if matches!(self.arena[key].kind, UnitKind::Service { .. }) {
                let _ = self.stop_service(key, Signal::SIGTERM);
            }
        }

        self.wait_for_services_stopped(Self::stop_timeout());

        for (_, unit) in self.arena.iter() {
            if let UnitKind::Mount {
                where_,
                mounted: true,
                ..
            } = &unit.kind
            {
                ghosttype_log("SHUTDOWN", &format!("Unmounting {where_}"));
                let _ = crate::mount_unit::unmount(where_);
            }
        }

        unsafe {
            libc::sync();
        }
        ghosttype_log("SHUTDOWN", "Shutdown sequence complete");
        Ok(())
    }

    fn stop_order(&self) -> Result<Vec<UnitKey>, ForgeError> {
        let waves = self.boot_waves()?;
        let mut order = Vec::new();
        for wave in waves.into_iter().rev() {
            for key in wave {
                if matches!(self.arena[key].kind, UnitKind::Service { .. }) {
                    order.push(key);
                }
            }
        }
        Ok(order)
    }

    pub fn status_snapshot(&self) -> Vec<ServiceStatus> {
        self.arena
            .iter()
            .filter(|(key, _)| self.active_units.contains(key))
            .map(|(_, unit)| {
                let kind = match &unit.kind {
                    UnitKind::Service { .. } => "service",
                    UnitKind::Socket { .. } => "socket",
                    UnitKind::Target { .. } => "target",
                    UnitKind::Device { .. } => "device",
                    UnitKind::Timer { .. } => "timer",
                    UnitKind::Mount { .. } => "mount",
                };
                let pid = match &unit.kind {
                    UnitKind::Service { pid, .. } => *pid,
                    _ => None,
                };
                ServiceStatus {
                    name: unit.name.to_string(),
                    kind: kind.to_string(),
                    state: format!("{:?}", unit.state),
                    pid,
                }
            })
            .collect()
    }

    pub fn on_child_exit(&mut self, pid: u32, status: i32) -> bool {
        let Some(key) = self.pid_index.remove(&pid) else {
            return false;
        };

        let exit_detail = if libc::WIFEXITED(status) {
            format!("exit code {}", libc::WEXITSTATUS(status))
        } else if libc::WIFSIGNALED(status) {
            format!("signal {}", libc::WTERMSIG(status))
        } else {
            "unknown status".to_string()
        };

        let (name, should_restart) = {
            let unit = &mut self.arena[key];
            unit.last_exit = Some(status);
            let failed = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) != 0
                || libc::WIFSIGNALED(status);

            let restart = if self.shutting_down {
                if let UnitKind::Service { pid, .. } = &mut unit.kind {
                    *pid = None;
                }
                false
            } else if let UnitKind::Service { restart, pid, .. } = &mut unit.kind {
                *pid = None;
                match restart {
                    RestartPolicy::Always => true,
                    RestartPolicy::OnFailure => failed,
                    RestartPolicy::No => false,
                }
            } else {
                false
            };

            unit.state = if failed {
                UnitState::Failed
            } else {
                UnitState::Dead
            };

            (unit.name.to_string(), restart)
        };

        ghosttype_log(
            "EXITED",
            &format!("'{}' (PID {}) terminated ({})", name, pid, exit_detail),
        );
        crate::journal::record(
            &name,
            4,
            format!("Service exited ({exit_detail})"),
            Some(pid),
        );

        if should_restart {
            self.pending_restarts.push_back(key);
        }

        true
    }

    pub fn reap_all_pending(&mut self) -> u32 {
        self.check_watchdogs();
        let mut reaped = 0;
        loop {
            let mut status: libc::c_int = 0;
            let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            if pid <= 0 {
                break;
            }
            if !self.on_child_exit(pid as u32, status) {
                ghosttype_log("CLEANUP", &format!("Reaped unmanaged orphan PID {}", pid));
            }
            reaped += 1;
        }
        reaped
    }
}

pub fn lock_state(state: &Arc<Mutex<ForgeState>>) -> MutexGuard<'_, ForgeState> {
    state.lock()
}

impl Default for ForgeState {
    fn default() -> Self {
        Self::new("multi-user".into(), PathBuf::from("/run/forge/log"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_target_state() -> ForgeState {
        let mut state = ForgeState::default();
        state
            .register_target(
                "multi-user".into(),
                vec![],
                vec!["base".into(), "child".into()],
            )
            .unwrap();
        state
            .register_service(
                "base".into(),
                "/bin/true".into(),
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                RestartPolicy::No,
                ServiceLimits::default(),
            )
            .unwrap();
        state
            .register_service(
                "child".into(),
                "/bin/true".into(),
                vec![],
                vec!["base".into()],
                vec![],
                vec![],
                vec![],
                RestartPolicy::No,
                ServiceLimits::default(),
            )
            .unwrap();
        state
    }

    #[test]
    fn boot_waves_respect_dependencies() {
        let mut state = sample_target_state();
        state.resolve_target_closure().unwrap();
        state.resolve_dependencies().unwrap();
        let waves = state.boot_waves().unwrap();
        assert_eq!(waves.len(), 2);
        assert_eq!(state.arena[waves[0][0]].name.as_ref(), "base");
        assert_eq!(state.arena[waves[1][0]].name.as_ref(), "child");
    }

    #[test]
    fn detects_dependency_cycles() {
        let mut state = ForgeState::default();
        state
            .register_target("multi-user".into(), vec![], vec!["a".into(), "b".into()])
            .unwrap();
        state
            .register_service(
                "a".into(),
                "/bin/true".into(),
                vec![],
                vec!["b".into()],
                vec![],
                vec![],
                vec![],
                RestartPolicy::No,
                ServiceLimits::default(),
            )
            .unwrap();
        state
            .register_service(
                "b".into(),
                "/bin/true".into(),
                vec![],
                vec!["a".into()],
                vec![],
                vec![],
                vec![],
                RestartPolicy::No,
                ServiceLimits::default(),
            )
            .unwrap();
        state.resolve_target_closure().unwrap();
        state.resolve_dependencies().unwrap();
        assert!(matches!(state.boot_waves(), Err(ForgeError::CycleDetected)));
    }

    #[test]
    fn need_implies_ordering_without_after() {
        let mut state = ForgeState::default();
        state
            .register_target("multi-user".into(), vec![], vec!["app".into()])
            .unwrap();
        state
            .register_service(
                "dep".into(),
                "/bin/true".into(),
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                RestartPolicy::No,
                ServiceLimits::default(),
            )
            .unwrap();
        state
            .register_service(
                "app".into(),
                "/bin/true".into(),
                vec![],
                vec![],
                vec!["dep".into()],
                vec![],
                vec![],
                RestartPolicy::No,
                ServiceLimits::default(),
            )
            .unwrap();
        state.resolve_target_closure().unwrap();
        state.resolve_dependencies().unwrap();
        let waves = state.boot_waves().unwrap();
        assert_eq!(
            waves.len(),
            2,
            "need without after should still order dep before app"
        );
        assert_eq!(state.arena[waves[0][0]].name.as_ref(), "dep");
        assert_eq!(state.arena[waves[1][0]].name.as_ref(), "app");
        assert!(state.is_required(*state.name_index.get("dep").expect("dep unit")));
    }
}
