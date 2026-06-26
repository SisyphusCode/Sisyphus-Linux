use crate::cgroup::ServiceLimits;
use crate::sandbox::SandboxConfig;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceType {
    #[default]
    Simple,
    Notify,
    #[serde(alias = "notify-reload")]
    NotifyReload,
    Oneshot,
    Dbus,
    Forking,
}

#[derive(Debug, Clone)]
pub struct ServiceManifest {
    pub name: String,
    pub exec: String,
    pub args: Vec<String>,
    pub after: Vec<String>,
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub sockets: Vec<String>,
    pub restart: super::forge::RestartPolicy,
    pub cgroup: ServiceLimits,
    pub service_type: ServiceType,
    pub environment: Vec<(String, String)>,
    pub exec_start_pre: Option<(String, Vec<String>)>,
    /// Required when service_type is Dbus — well-known bus name to await.
    pub bus_name: Option<String>,
    pub sandbox: SandboxConfig,
    /// Parsed from Service User=
    pub user: Option<String>,
    /// Parsed from Service Group=
    pub group: Option<String>,
    /// Parsed from Service WorkingDirectory=
    pub working_directory: Option<String>,
    /// OpenRC runlevels this service may belong to (empty = all).
    pub runlevels: Vec<String>,
    pub timeout_secs: Option<u64>,
    pub pidfile: Option<String>,
    /// Virtual names declared with `provide = [...]` (OpenRC-style providers).
    pub provides: Vec<String>,
    /// Custom stop command; takes precedence over `stop_signal`.
    pub stop_cmd: Option<(String, Vec<String>)>,
    /// Signal name to send on graceful stop (e.g. `"SIGTERM"`). Default: SIGTERM.
    pub stop_signal: Option<String>,
    /// Custom reload command; runs without stopping the service.
    pub reload_cmd: Option<(String, Vec<String>)>,
    /// Signal name to send on reload (e.g. `"SIGHUP"`). Default: SIGHUP.
    pub reload_signal: Option<String>,
    /// Watchdog timeout in microseconds (from WatchdogSec or WATCHDOG_USEC).
    pub watchdog_usec: Option<u64>,
}

impl ServiceManifest {
    pub fn new(name: impl Into<String>, exec: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            exec: exec.into(),
            args: Vec::new(),
            after: Vec::new(),
            requires: Vec::new(),
            wants: Vec::new(),
            sockets: Vec::new(),
            restart: super::forge::RestartPolicy::default(),
            cgroup: ServiceLimits::default(),
            service_type: ServiceType::default(),
            environment: Vec::new(),
            exec_start_pre: None,
            bus_name: None,
            sandbox: SandboxConfig::default(),
            user: None,
            group: None,
            working_directory: None,
            runlevels: Vec::new(),
            timeout_secs: None,
            pidfile: None,
            provides: Vec::new(),
            stop_cmd: None,
            stop_signal: None,
            reload_cmd: None,
            reload_signal: None,
            watchdog_usec: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceManifest {
    pub name: String,
    pub path: std::path::PathBuf,
    pub service: String,
    pub after: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TimerManifest {
    pub name: String,
    pub unit: String,
    pub after: Vec<String>,
    pub on_boot_sec: Option<f64>,
    #[allow(dead_code)]
    pub on_unit_active_sec: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct MountManifest {
    pub name: String,
    pub what: String,
    pub where_: String,
    pub fstype: String,
    pub options: String,
    pub after: Vec<String>,
}
