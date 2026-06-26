//! Native OpenRC-style service definitions for The Forge
//!
//! This module provides a pure Rust implementation of OpenRC-like functionality,
//! replacing the systemd compatibility layer with a simpler, more direct approach
//! that maintains The Forge's performance and safety guarantees.
//!
//! Key differences from systemd compatibility:
//! - Simpler configuration format inspired by OpenRC
//! - Direct Rust implementation (no shell scripts)
//! - OpenRC-style dependency keywords: need, use, before, after
//! - Runlevels instead of targets (though targets are still supported)
//! - Pure Rust process supervision

use crate::cgroup::ServiceLimits;
use crate::sandbox::SandboxConfig;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::forge::{ForgeState, RestartPolicy};
use super::ghosttype_log;
use super::manifest::{ServiceManifest, ServiceType as ManifestServiceType};

/// Native service configuration - pure Rust, OpenRC-inspired
#[derive(Debug, Clone, Deserialize)]
pub struct NativeService {
    /// Service name
    pub name: String,

    /// Human-readable description (logged at registration time)
    #[serde(default)]
    pub description: Option<String>,

    /// Command to execute (path to binary)
    pub command: String,

    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,

    // OpenRC-style dependencies
    /// Hard dependencies - must start before this service
    #[serde(default)]
    pub need: Vec<String>,

    /// Soft dependencies - nice to have started before this service
    #[serde(default, rename = "use")]
    pub use_: Vec<String>,

    /// This service must start before these services
    #[serde(default)]
    pub before: Vec<String>,

    /// These services must start before this service
    #[serde(default)]
    pub after: Vec<String>,

    /// Runlevels this service belongs to
    #[serde(default)]
    pub runlevels: Vec<String>,

    // Service behavior
    /// Restart policy
    #[serde(default, rename = "restart")]
    pub restart_policy: RestartPolicy,

    /// Service type (simple, forking, oneshot, etc.)
    #[serde(default, rename = "type")]
    pub service_type: ServiceType,

    /// Socket units that activate this service
    #[serde(default)]
    pub sockets: Vec<String>,

    /// Command to run before main exec
    #[serde(rename = "exec-start-pre", default)]
    pub exec_start_pre: Option<String>,

    /// D-Bus well-known name (required for dbus/notify services)
    #[serde(rename = "bus-name", default)]
    pub bus_name: Option<String>,

    // Process management
    /// User to run as
    #[serde(default)]
    pub user: Option<String>,

    /// Group to run as
    #[serde(default)]
    pub group: Option<String>,

    /// Working directory
    #[serde(default)]
    pub working_directory: Option<String>,

    /// Environment variables
    #[serde(default)]
    pub environment: HashMap<String, String>,

    /// Environment file to load
    #[serde(default)]
    pub environment_file: Option<String>,

    /// Timeout for starting the service
    #[serde(default)]
    pub timeout_secs: Option<u64>,

    /// PID file to write
    #[serde(default)]
    pub pidfile: Option<String>,

    /// Cgroup limits
    #[serde(default)]
    pub cgroup: ServiceLimits,

    /// Sandbox configuration
    #[serde(default)]
    pub sandbox: SandboxConfig,

    // OpenRC-specific features
    /// Supervision mode (restart on crash)
    #[serde(default)]
    pub supervise: bool,

    /// Crash on start (fail if service crashes during startup)
    #[serde(default)]
    pub crash_on_start: bool,

    // === New OpenRC gap-filling fields ===
    /// Virtual names this service provides (OpenRC `provide` keyword).
    /// Other services can depend on these names rather than the concrete unit name.
    #[serde(default)]
    pub provides: Vec<String>,

    /// Custom stop command. When set, this runs instead of sending a signal.
    /// Example: `stop-cmd = "/usr/bin/pg_ctl stop -D /var/lib/postgres"`
    #[serde(rename = "stop-cmd", default)]
    pub stop_cmd: Option<String>,

    /// Signal to send on graceful stop. Default: `SIGTERM`.
    /// Only used when `stop-cmd` is not set.
    /// Example: `stop-signal = "SIGQUIT"`
    #[serde(rename = "stop-signal", default)]
    pub stop_signal: Option<String>,

    /// Custom reload command. Runs without stopping the service.
    /// Example: `reload-cmd = "/usr/sbin/nginx -s reload"`
    #[serde(rename = "reload-cmd", default)]
    pub reload_cmd: Option<String>,

    /// Signal to send on reload. Default: `SIGHUP`.
    /// Only used when `reload-cmd` is not set.
    /// Example: `reload-signal = "SIGUSR2"`
    #[serde(rename = "reload-signal", default)]
    pub reload_signal: Option<String>,

    /// Watchdog timeout, e.g. "30s" or microseconds number.
    #[serde(rename = "watchdog", default)]
    pub watchdog: Option<String>,
}

impl Default for NativeService {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: None,
            command: String::new(),
            args: Vec::new(),
            need: Vec::new(),
            use_: Vec::new(),
            before: Vec::new(),
            after: Vec::new(),
            runlevels: Vec::new(),
            restart_policy: RestartPolicy::default(),
            service_type: ServiceType::Simple,
            sockets: Vec::new(),
            exec_start_pre: None,
            bus_name: None,
            user: None,
            group: None,
            working_directory: None,
            environment: HashMap::new(),
            environment_file: None,
            timeout_secs: None,
            pidfile: None,
            cgroup: ServiceLimits::default(),
            sandbox: SandboxConfig::default(),
            supervise: false,
            crash_on_start: false,
            provides: Vec::new(),
            stop_cmd: None,
            stop_signal: None,
            reload_cmd: None,
            reload_signal: None,
            watchdog: None,
        }
    }
}

/// Service type for native services
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceType {
    #[default]
    Simple,
    Forking,
    Oneshot,
    Dbus,
    Notify,
    #[serde(alias = "notify-reload")]
    NotifyReload,
}

impl std::fmt::Display for ServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceType::Simple => write!(f, "simple"),
            ServiceType::Forking => write!(f, "forking"),
            ServiceType::Oneshot => write!(f, "oneshot"),
            ServiceType::Dbus => write!(f, "dbus"),
            ServiceType::Notify => write!(f, "notify"),
            ServiceType::NotifyReload => write!(f, "notify-reload"),
        }
    }
}

/// Native socket unit (socket activation)
#[derive(Debug, Clone, Deserialize)]
pub struct NativeSocket {
    pub name: String,
    #[serde(default)]
    pub listen: Vec<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub after: Vec<String>,
}

/// Native target definition (similar to OpenRC runlevels)
#[derive(Debug, Clone, Deserialize)]
pub struct NativeTarget {
    pub name: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub wants: Vec<String>,
}

impl Default for NativeTarget {
    fn default() -> Self {
        Self {
            name: String::new(),
            requires: Vec::new(),
            wants: Vec::new(),
        }
    }
}

/// Native unit file that can contain a service, socket, or target
#[derive(Debug)]
pub enum NativeUnit {
    Service(NativeService),
    Socket(NativeSocket),
    Target(NativeTarget),
}

fn native_unit_kind(path: &Path) -> &'static str {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem.ends_with(".socket") {
        "socket"
    } else if stem.ends_with(".target") {
        "target"
    } else {
        "service"
    }
}

fn parse_native_unit(path: &Path, content: &str) -> Result<NativeUnit, String> {
    match native_unit_kind(path) {
        "socket" => {
            let socket: NativeSocket = toml::from_str(content)
                .map_err(|e| format!("Invalid native socket {}: {e}", path.display()))?;
            Ok(NativeUnit::Socket(socket))
        }
        "target" => {
            let target: NativeTarget = toml::from_str(content)
                .map_err(|e| format!("Invalid native target {}: {e}", path.display()))?;
            Ok(NativeUnit::Target(target))
        }
        _ => {
            let service: NativeService = toml::from_str(content)
                .map_err(|e| format!("Invalid native service {}: {e}", path.display()))?;
            Ok(NativeUnit::Service(service))
        }
    }
}

/// Load all native units from a directory
pub fn load_native_units(state: &mut ForgeState, dir: &Path, lenient: bool) -> Result<(), String> {
    if !dir.exists() {
        ghosttype_log(
            "NATIVE",
            &format!("Native units directory not found: {}", dir.display()),
        );
        return Ok(());
    }

    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("Cannot read {}: {}", dir.display(), e))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext == "toml")
                    .unwrap_or(false)
        })
        .collect();

    paths.sort();

    let mut errors = Vec::new();
    let mut loaded = 0;
    let mut before_edges: Vec<(String, String)> = Vec::new();

    for path in paths {
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                if lenient {
                    ghosttype_log("WARN", &format!("Skipping {}: {}", path.display(), e));
                    errors.push(e.to_string());
                    continue;
                }
                return Err(e.to_string());
            }
        };

        let unit: NativeUnit = match parse_native_unit(&path, &content) {
            Ok(u) => u,
            Err(e) => {
                if lenient {
                    ghosttype_log("WARN", &format!("Skipping {}: {}", path.display(), e));
                    errors.push(e);
                    continue;
                }
                return Err(e);
            }
        };

        match unit {
            NativeUnit::Service(service) => {
                for dep in &service.before {
                    before_edges.push((service.name.clone(), dep.clone()));
                }
                match register_native_service(state, service) {
                    Ok(()) => loaded += 1,
                    Err(e) => {
                        if lenient {
                            ghosttype_log("WARN", &format!("Skipping {}: {}", path.display(), e));
                            errors.push(e);
                        } else {
                            return Err(e);
                        }
                    }
                }
            }
            NativeUnit::Socket(socket) => match register_native_socket(state, socket) {
                Ok(()) => loaded += 1,
                Err(e) => {
                    if lenient {
                        ghosttype_log("WARN", &format!("Skipping {}: {}", path.display(), e));
                        errors.push(e);
                    } else {
                        return Err(e);
                    }
                }
            },
            NativeUnit::Target(target) => match register_native_target(state, target) {
                Ok(()) => loaded += 1,
                Err(e) => {
                    if lenient {
                        ghosttype_log("WARN", &format!("Skipping {}: {}", path.display(), e));
                        errors.push(e);
                    } else {
                        return Err(e);
                    }
                }
            },
        }
    }

    for (from, to) in before_edges {
        if let Err(e) = state.prepend_ordering(&to, &from) {
            let msg = format!("before ordering {from} -> {to}: {e}");
            if lenient {
                ghosttype_log("WARN", &msg);
                errors.push(msg);
            } else {
                return Err(msg);
            }
        }
    }

    ghosttype_log(
        "NATIVE",
        &format!("Loaded {loaded} native unit(s) from {}", dir.display()),
    );

    if lenient && !errors.is_empty() {
        return Err(format!(
            "{} native unit(s) skipped due to errors",
            errors.len()
        ));
    }

    Ok(())
}

/// Register a native service with the Forge state
pub fn register_native_service(
    state: &mut ForgeState,
    service: NativeService,
) -> Result<(), String> {
    let name = service.name.clone();
    let description = service.description.clone();

    let restart = if service.supervise {
        if service.crash_on_start {
            RestartPolicy::OnFailure
        } else {
            RestartPolicy::Always
        }
    } else {
        service.restart_policy
    };

    let exec_start_pre = service
        .exec_start_pre
        .map(|cmd| super::systemd::parse_exec_start(&cmd))
        .transpose()
        .map_err(|e| format!("exec-start-pre for '{name}': {e}"))?;

    let stop_cmd = service
        .stop_cmd
        .map(|cmd| super::systemd::parse_exec_start(&cmd))
        .transpose()
        .map_err(|e| format!("stop-cmd for '{name}': {e}"))?;

    let reload_cmd = service
        .reload_cmd
        .map(|cmd| super::systemd::parse_exec_start(&cmd))
        .transpose()
        .map_err(|e| format!("reload-cmd for '{name}': {e}"))?;

    let mut manifest = ServiceManifest::new(service.name.clone(), service.command);
    manifest.args = service.args;
    manifest.after = service.after;
    manifest.requires = service.need;
    manifest.wants = service.use_;
    manifest.sockets = service.sockets;
    manifest.restart = restart;
    manifest.cgroup = service.cgroup;
    manifest.service_type = map_service_type(service.service_type);
    let mut environment: Vec<(String, String)> = service.environment.into_iter().collect();
    if let Some(ref path) = service.environment_file {
        match crate::environment::load_environment_file(std::path::Path::new(path)) {
            Ok(vars) => {
                for (k, v) in vars {
                    environment.push((k, v));
                }
            }
            Err(e) => {
                return Err(format!("Failed to load environment_file for '{name}': {e}"));
            }
        }
    }
    manifest.environment = environment;
    manifest.exec_start_pre = exec_start_pre;
    manifest.bus_name = service.bus_name;
    manifest.sandbox = service.sandbox;
    manifest.user = service.user;
    manifest.group = service.group;
    manifest.working_directory = service.working_directory;
    manifest.runlevels = service.runlevels;
    manifest.timeout_secs = service.timeout_secs;
    manifest.pidfile = service.pidfile;
    manifest.provides = service.provides;
    manifest.stop_cmd = stop_cmd;
    manifest.stop_signal = service.stop_signal;
    manifest.reload_cmd = reload_cmd;
    manifest.reload_signal = service.reload_signal;
    manifest.watchdog_usec = parse_watchdog(service.watchdog.as_deref());

    state
        .register_service_manifest(manifest)
        .map_err(|e| format!("Failed to register native service '{name}': {e}"))?;

    let desc_note = description
        .as_deref()
        .map(|d| format!(" — {d}"))
        .unwrap_or_default();
    ghosttype_log("NATIVE", &format!("Registered service '{name}'{desc_note}"));

    Ok(())
}

/// Register a native socket unit with the Forge state
pub fn register_native_socket(state: &mut ForgeState, socket: NativeSocket) -> Result<(), String> {
    let name = socket.name.clone();
    state
        .register_socket(name.clone(), socket.listen, socket.service, socket.after)
        .map_err(|e| format!("Failed to register native socket '{name}': {e}"))?;
    ghosttype_log("NATIVE", &format!("Registered socket '{name}'"));
    Ok(())
}

/// Register a native target with the Forge state
pub fn register_native_target(state: &mut ForgeState, target: NativeTarget) -> Result<(), String> {
    let name = target.name.clone();
    state
        .register_target(target.name, target.requires, target.wants)
        .map_err(|e| format!("Failed to register native target '{name}': {e}"))?;
    ghosttype_log("NATIVE", &format!("Registered target '{name}'"));
    Ok(())
}

/// Map native service type to Forge service type
fn map_service_type(st: ServiceType) -> ManifestServiceType {
    match st {
        ServiceType::Simple => ManifestServiceType::Simple,
        ServiceType::Forking => ManifestServiceType::Forking,
        ServiceType::Oneshot => ManifestServiceType::Oneshot,
        ServiceType::Dbus => ManifestServiceType::Dbus,
        ServiceType::Notify => ManifestServiceType::Notify,
        ServiceType::NotifyReload => ManifestServiceType::NotifyReload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_temp_service(content: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("forge-native-test-{stamp}.service.toml"));
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Verifies that a well-formed native service TOML file round-trips through
    /// the parser correctly, including optional and new fields.
    #[test]
    fn parses_native_service_toml() {
        let content = r#"
name = "test-service"
description = "A test service"
command = "/usr/bin/test"
args = ["--arg1", "value1"]
need = ["localmount", "devfs"]
use = ["logger"]
runlevels = ["default"]
user = "testuser"
group = "testgroup"
supervise = true
provides = ["net", "network"]
stop-cmd = "/usr/sbin/nginx -s quit"
stop-signal = "SIGQUIT"
reload-cmd = "/usr/sbin/nginx -s reload"
reload-signal = "SIGUSR2"
"#;

        let path = write_temp_service(content);
        let raw = std::fs::read_to_string(&path).unwrap();
        let service: NativeService = toml::from_str(&raw).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(service.name, "test-service");
        assert_eq!(service.description.as_deref(), Some("A test service"));
        assert_eq!(service.command, "/usr/bin/test");
        assert_eq!(service.args, vec!["--arg1", "value1"]);
        assert_eq!(service.need, vec!["localmount", "devfs"]);
        assert_eq!(service.use_, vec!["logger"]);
        assert_eq!(service.runlevels, vec!["default"]);
        assert_eq!(service.user.as_deref(), Some("testuser"));
        assert_eq!(service.group.as_deref(), Some("testgroup"));
        assert!(service.supervise);
        assert_eq!(service.provides, vec!["net", "network"]);
        assert_eq!(service.stop_cmd.as_deref(), Some("/usr/sbin/nginx -s quit"));
        assert_eq!(service.stop_signal.as_deref(), Some("SIGQUIT"));
        assert_eq!(
            service.reload_cmd.as_deref(),
            Some("/usr/sbin/nginx -s reload")
        );
        assert_eq!(service.reload_signal.as_deref(), Some("SIGUSR2"));
    }
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
