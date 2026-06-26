use super::dropin::apply_systemd_dropins;
use super::dropin::apply_toml_dropins;
use super::forge::{ForgeState, RestartPolicy};
use super::ghosttype_log;
use super::manifest::{DeviceManifest, MountManifest, ServiceManifest, ServiceType, TimerManifest};
use super::systemd::{
    collect_service_environment, map_restart, map_service_type, normalize_unit_name,
    parse_exec_start, unit_name_from_path, SystemdUnit,
};
use crate::cgroup::ServiceLimits;
use crate::environment::load_environment_file;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
struct CgroupSpec {
    #[serde(default, rename = "memory-max")]
    memory_max: Option<String>,
    #[serde(default, rename = "tasks-max")]
    tasks_max: Option<u64>,
}

impl From<CgroupSpec> for ServiceLimits {
    fn from(value: CgroupSpec) -> Self {
        ServiceLimits {
            memory_max: value.memory_max,
            tasks_max: value.tasks_max,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct EnvironmentSpec {
    #[serde(default, rename = "environment-file")]
    environment_file: Option<String>,
    #[serde(flatten)]
    vars: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct RawUnitFile {
    service: Option<ServiceSpec>,
    socket: Option<SocketSpec>,
    target: Option<TargetSpec>,
    device: Option<DeviceSpec>,
    timer: Option<TimerSpec>,
    mount: Option<MountSpec>,
}

#[derive(Debug, Deserialize)]
struct ServiceSpec {
    name: String,
    exec: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    after: Vec<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    wants: Vec<String>,
    #[serde(default)]
    sockets: Vec<String>,
    #[serde(default)]
    restart: RestartPolicy,
    #[serde(default)]
    cgroup: CgroupSpec,
    #[serde(default, rename = "type")]
    service_type: ServiceType,
    #[serde(default)]
    environment: EnvironmentSpec,
    #[serde(default, rename = "exec-start-pre")]
    exec_start_pre: Option<String>,
    #[serde(default, rename = "bus-name")]
    bus_name: Option<String>,
    #[serde(default)]
    sandbox: SandboxSpec,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default, rename = "working-directory")]
    working_directory: Option<String>,
    #[serde(default)]
    provides: Vec<String>,
    #[serde(default, rename = "stop-cmd")]
    stop_cmd: Option<String>,
    #[serde(default, rename = "stop-signal")]
    stop_signal: Option<String>,
    #[serde(default, rename = "reload-cmd")]
    reload_cmd: Option<String>,
    #[serde(default, rename = "reload-signal")]
    reload_signal: Option<String>,
    #[serde(default, rename = "watchdog")]
    watchdog: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SandboxSpec {
    #[serde(default, rename = "private-tmp")]
    private_tmp: bool,
    #[serde(default, rename = "protect-system")]
    protect_system: bool,
    #[serde(default, rename = "private-devices")]
    private_devices: bool,
    #[serde(default, rename = "no-new-privileges")]
    no_new_privileges: bool,
}

impl From<SandboxSpec> for crate::sandbox::SandboxConfig {
    fn from(value: SandboxSpec) -> Self {
        Self {
            private_tmp: value.private_tmp,
            protect_system: value.protect_system,
            private_devices: value.private_devices,
            no_new_privileges: value.no_new_privileges,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TimerSpec {
    name: String,
    unit: String,
    #[serde(default)]
    after: Vec<String>,
    #[serde(default, rename = "on-boot-sec")]
    on_boot_sec: Option<f64>,
    #[serde(default, rename = "on-unit-active-sec")]
    on_unit_active_sec: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct MountSpec {
    name: String,
    what: String,
    #[serde(default, rename = "where")]
    where_: String,
    #[serde(default)]
    fstype: String,
    #[serde(default)]
    options: String,
    #[serde(default)]
    after: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SocketSpec {
    name: String,
    listen: Vec<String>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    after: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TargetSpec {
    name: String,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    wants: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceSpec {
    name: String,
    path: String,
    service: String,
    #[serde(default)]
    after: Vec<String>,
}

pub fn load_all_units(
    state: &mut ForgeState,
    toml_dir: &Path,
    systemd_dir: &Path,
    lenient: bool,
) -> Result<(), String> {
    load_toml_units(state, toml_dir, lenient)?;
    load_systemd_units(state, systemd_dir, lenient)?;

    // Mirror more of systemd: when requested, also load units from the installed system locations.
    // Use e.g. FORGE_IMPORT_SYSTEM_UNITS=1 to consume real dbus-broker.service, systemd-*.service etc.
    if std::env::var("FORGE_IMPORT_SYSTEM_UNITS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        for extra in ["/usr/lib/systemd/system", "/etc/systemd/system"] {
            let p = Path::new(extra);
            if p.exists() {
                let _ = load_systemd_units(state, p, true);
            }
        }
    }
    Ok(())
}

fn load_toml_units(state: &mut ForgeState, dir: &Path, lenient: bool) -> Result<(), String> {
    if !dir.exists() {
        ghosttype_log(
            "UNITS",
            &format!("TOML unit dir not found, skipping: {}", dir.display()),
        );
        return Ok(());
    }

    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("Cannot read {}: {}", dir.display(), e))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect();
    paths.sort();

    let before = state.len();
    let mut errors = Vec::new();
    for path in paths {
        if let Err(err) = load_toml_unit_file(state, dir, &path) {
            if lenient {
                ghosttype_log("WARN", &format!("Skipping {}: {err}", path.display()));
                errors.push(err);
                continue;
            }
            return Err(err);
        }
    }

    let loaded = state.len().saturating_sub(before);
    ghosttype_log(
        "UNITS",
        &format!("Loaded {loaded} TOML unit(s) from {}", dir.display()),
    );
    if lenient && !errors.is_empty() {
        return Err(format!(
            "{} unit file(s) skipped due to errors",
            errors.len()
        ));
    }
    Ok(())
}

fn load_toml_unit_file(state: &mut ForgeState, dir: &Path, path: &Path) -> Result<(), String> {
    let raw =
        fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    let unit: RawUnitFile =
        toml::from_str(&raw).map_err(|e| format!("Invalid unit {}: {}", path.display(), e))?;

    let tables = [
        unit.service.is_some(),
        unit.socket.is_some(),
        unit.target.is_some(),
        unit.device.is_some(),
        unit.timer.is_some(),
        unit.mount.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if tables != 1 {
        return Err(format!(
                "Unit {} must contain exactly one of [service], [socket], [target], [device], [timer], or [mount]",
                path.display()
            ));
    }

    if let Some(spec) = unit.service {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(String::from)
            .unwrap_or_else(|| spec.name.clone());
        let mut manifest = service_from_spec(spec)?;
        apply_toml_dropins(dir, &stem, &mut manifest);
        state
            .register_service_manifest(manifest)
            .map_err(|e| e.to_string())?;
    } else if let Some(spec) = unit.socket {
        state
            .register_socket(spec.name, spec.listen, spec.service, spec.after)
            .map_err(|e| e.to_string())?;
    } else if let Some(spec) = unit.target {
        state
            .register_target(spec.name, spec.requires, spec.wants)
            .map_err(|e| e.to_string())?;
    } else if let Some(spec) = unit.device {
        state
            .register_device(DeviceManifest {
                name: spec.name,
                path: PathBuf::from(spec.path),
                service: spec.service,
                after: spec.after,
            })
            .map_err(|e| e.to_string())?;
    } else if let Some(spec) = unit.timer {
        state
            .register_timer(TimerManifest {
                name: spec.name,
                unit: spec.unit,
                after: spec.after,
                on_boot_sec: spec.on_boot_sec,
                on_unit_active_sec: spec.on_unit_active_sec,
            })
            .map_err(|e| e.to_string())?;
    } else if let Some(spec) = unit.mount {
        state
            .register_mount(MountManifest {
                name: spec.name,
                what: spec.what,
                where_: spec.where_,
                fstype: if spec.fstype.is_empty() {
                    "auto".into()
                } else {
                    spec.fstype
                },
                options: if spec.options.is_empty() {
                    "defaults".into()
                } else {
                    spec.options
                },
                after: spec.after,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn service_from_spec(spec: ServiceSpec) -> Result<ServiceManifest, String> {
    let mut environment: Vec<(String, String)> = spec.environment.vars.into_iter().collect();
    if let Some(file) = spec.environment.environment_file {
        environment.extend(load_environment_file(Path::new(&file))?);
    }
    let exec_start_pre = if let Some(line) = spec.exec_start_pre {
        Some(parse_exec_start(&line)?)
    } else {
        None
    };
    let stop_cmd = spec.stop_cmd.map(|c| parse_exec_start(&c)).transpose()?;
    let reload_cmd = spec.reload_cmd.map(|c| parse_exec_start(&c)).transpose()?;

    Ok(ServiceManifest {
        name: spec.name,
        exec: spec.exec,
        args: spec.args,
        after: spec.after,
        requires: spec.requires,
        wants: spec.wants,
        sockets: spec.sockets,
        restart: spec.restart,
        cgroup: spec.cgroup.into(),
        service_type: spec.service_type,
        environment,
        exec_start_pre,
        bus_name: spec.bus_name,
        sandbox: spec.sandbox.into(),
        user: spec.user,
        group: spec.group,
        working_directory: spec.working_directory,
        runlevels: Vec::new(),
        timeout_secs: None,
        pidfile: None,
        provides: spec.provides,
        stop_cmd,
        stop_signal: spec.stop_signal,
        reload_cmd,
        reload_signal: spec.reload_signal,
        watchdog_usec: parse_watchdog(spec.watchdog.as_deref()),
    })
}

fn parse_watchdog(s: Option<&str>) -> Option<u64> {
    let s = s?;
    let s = s.trim();
    if let Ok(us) = s.parse::<u64>() {
        return Some(us);
    }
    if let Some(num) = s.strip_suffix('s').or_else(|| s.strip_suffix("sec")) {
        if let Ok(secs) = num.trim().parse::<u64>() {
            return Some(secs * 1_000_000);
        }
    }
    if let Some(num) = s.strip_suffix('m').or_else(|| s.strip_suffix("min")) {
        if let Ok(mins) = num.trim().parse::<u64>() {
            return Some(mins * 60 * 1_000_000);
        }
    }
    None
}

fn load_systemd_units(state: &mut ForgeState, dir: &Path, lenient: bool) -> Result<(), String> {
    if !dir.exists() {
        ghosttype_log(
            "UNITS",
            &format!("systemd unit dir not found, skipping: {}", dir.display()),
        );
        return Ok(());
    }

    let before = state.len();
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("Cannot read {}: {}", dir.display(), e))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(ext, "service" | "socket" | "target" | "timer" | "mount")
                })
        })
        .collect();
    paths.sort();

    let mut errors = Vec::new();
    for path in paths {
        if let Err(err) = load_systemd_unit_file(state, dir, &path) {
            if lenient {
                ghosttype_log("WARN", &format!("Skipping {}: {err}", path.display()));
                errors.push(err);
                continue;
            }
            return Err(err);
        }
    }

    let loaded = state.len().saturating_sub(before);
    ghosttype_log(
        "UNITS",
        &format!("Imported {loaded} systemd unit(s) from {}", dir.display()),
    );
    if lenient && !errors.is_empty() {
        return Err(format!(
            "{} systemd unit(s) skipped due to errors",
            errors.len()
        ));
    }
    Ok(())
}

fn load_systemd_unit_file(state: &mut ForgeState, dir: &Path, path: &Path) -> Result<(), String> {
    let raw =
        fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    let mut unit = SystemdUnit::parse(&raw);
    let name = unit_name_from_path(path);
    apply_systemd_dropins(dir, &name, &mut unit);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "service" => {
            let exec_start = unit
                .get("Service", "ExecStart")
                .ok_or_else(|| format!("{} missing ExecStart", path.display()))?;
            let (exec, args) = parse_exec_start(&exec_start)?;
            let restart = unit
                .get("Service", "Restart")
                .map(|v| map_restart(&v))
                .unwrap_or_default();
            let service_type = unit
                .get("Service", "Type")
                .map(|v| map_service_type(&v))
                .unwrap_or_default();
            let bus_name = unit.get("Service", "BusName");
            let after = unit.list("Unit", "After");
            let requires = unit.list("Unit", "Requires");
            let wants = unit.list("Unit", "Wants");
            let sockets = unit.list("Service", "Sockets");
            let environment = collect_service_environment(&unit)?;
            let exec_start_pre = unit
                .get("Service", "ExecStartPre")
                .map(|line| parse_exec_start(&line))
                .transpose()?;

            let limits = ServiceLimits {
                memory_max: unit.get("Service", "MemoryMax"),
                tasks_max: unit.get("Service", "TasksMax").and_then(|v| v.parse().ok()),
            };

            let sandbox = crate::sandbox::SandboxConfig::from_systemd_unit(&unit);

            let user = unit.get("Service", "User");
            let group = unit.get("Service", "Group");
            let working_directory = unit.get("Service", "WorkingDirectory");
            let stop_cmd = unit
                .get("Service", "ExecStop")
                .map(|line| parse_exec_start(&line))
                .transpose()?;
            let stop_signal = unit.get("Service", "KillSignal");
            let reload_cmd = unit
                .get("Service", "ExecReload")
                .map(|line| parse_exec_start(&line))
                .transpose()?;
            let watchdog = unit.get("Service", "WatchdogSec");

            state
                .register_service_manifest(ServiceManifest {
                    name,
                    exec,
                    args,
                    after,
                    requires,
                    wants,
                    sockets,
                    restart,
                    cgroup: limits,
                    service_type,
                    environment,
                    exec_start_pre,
                    bus_name,
                    sandbox,
                    user,
                    group,
                    working_directory,
                    runlevels: Vec::new(),
                    timeout_secs: None,
                    pidfile: None,
                    provides: Vec::new(),
                    stop_cmd,
                    stop_signal,
                    reload_cmd,
                    reload_signal: None,
                    watchdog_usec: parse_watchdog(watchdog.as_deref()),
                })
                .map_err(|e| e.to_string())?;
        }
        "socket" => {
            let mut listen = unit.values("Socket", "ListenStream");
            listen.extend(unit.values("Socket", "ListenSequentialPacket"));
            for nl in unit.values("Socket", "ListenNetlink") {
                listen.push(format!("netlink:{nl}"));
            }
            let service = unit
                .get("Socket", "Service")
                .as_deref()
                .map(super::systemd::normalize_unit_name);
            let after = unit.list("Unit", "After");
            state
                .register_socket(name, listen, service, after)
                .map_err(|e| e.to_string())?;
        }
        "target" => {
            let requires = unit.list("Unit", "Requires");
            let wants = unit.list("Unit", "Wants");
            state
                .register_target(name, requires, wants)
                .map_err(|e| e.to_string())?;
        }
        "timer" => {
            let unit_name = unit
                .get("Timer", "Unit")
                .map(|v| normalize_unit_name(&v))
                .unwrap_or_else(|| normalize_unit_name(&format!("{name}.service")));
            let on_boot_sec = unit
                .get("Timer", "OnBootSec")
                .and_then(|v| parse_systemd_duration(&v));
            let after = unit.list("Unit", "After");
            state
                .register_timer(TimerManifest {
                    name,
                    unit: unit_name,
                    after,
                    on_boot_sec,
                    on_unit_active_sec: None,
                })
                .map_err(|e| e.to_string())?;
        }
        "mount" => {
            let what = unit
                .get("Mount", "What")
                .ok_or_else(|| format!("{} missing What", path.display()))?;
            let where_ = unit
                .get("Mount", "Where")
                .ok_or_else(|| format!("{} missing Where", path.display()))?;
            let fstype = unit.get("Mount", "Type").unwrap_or_else(|| "auto".into());
            let options = unit
                .get("Mount", "Options")
                .unwrap_or_else(|| "defaults".into());
            let after = unit.list("Unit", "After");
            state
                .register_mount(MountManifest {
                    name,
                    what,
                    where_,
                    fstype,
                    options,
                    after,
                })
                .map_err(|e| e.to_string())?;
        }
        _ => {}
    }
    Ok(())
}

pub fn read_default_target(path: &Path) -> Result<String, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(super::systemd::normalize_unit_name(raw.trim()))
}

/// Parse systemd time spans like `5min`, `30s`, `1h 30min`.
fn parse_systemd_duration(raw: &str) -> Option<f64> {
    let mut total = 0.0f64;
    for token in raw.split_whitespace() {
        let (num, suffix) = token.split_at(
            token
                .find(|c: char| !c.is_ascii_digit() && c != '.')
                .unwrap_or(token.len()),
        );
        let value: f64 = num.parse().ok()?;
        let secs = match suffix {
            "s" | "sec" | "secs" | "second" | "seconds" => value,
            "ms" | "msec" | "msecs" => value / 1000.0,
            "m" | "min" | "mins" | "minute" | "minutes" => value * 60.0,
            "h" | "hr" | "hrs" | "hour" | "hours" => value * 3600.0,
            "d" | "day" | "days" => value * 86400.0,
            "" => value,
            _ => return None,
        };
        total += secs;
    }
    Some(total)
}
