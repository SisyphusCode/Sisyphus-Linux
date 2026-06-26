use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTROL_SOCKET: &str = "/run/forge/control.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum ControlRequest {
    Status,
    BootProfile,
    Logs {
        name: String,
        #[serde(default)]
        tail: Option<usize>,
    },
    Start {
        name: String,
    },
    Stop {
        name: String,
    },
    Restart {
        name: String,
    },
    Reload {
        name: String,
    },
    ActivateTarget {
        name: String,
    },
    Enable {
        service: String,
        #[serde(default)]
        runlevel: Option<String>,
    },
    Disable {
        service: String,
        #[serde(default)]
        runlevel: Option<String>,
    },
    Service {
        name: String,
    },
    RcUpdateAdd {
        service: String,
        runlevel: String,
    },
    RcUpdateDel {
        service: String,
        runlevel: String,
    },
    RcUpdateShow {
        #[serde(default)]
        runlevel: Option<String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub source: String,
    pub ts: Option<u128>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveTiming {
    pub wave: usize,
    pub services: Vec<String>,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootProfileReport {
    pub total_boot_ms: u128,
    pub active_target: String,
    pub waves: Vec<WaveTiming>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ControlResponse {
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        services: Option<Vec<ServiceStatus>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        profile: Option<BootProfileReport>,
        #[serde(skip_serializing_if = "Option::is_none")]
        logs: Option<Vec<LogLine>>,
    },
    Error {
        message: String,
    },
}

impl ControlResponse {
    pub fn ok_message(message: impl Into<String>) -> Self {
        Self::Ok {
            message: Some(message.into()),
            services: None,
            profile: None,
            logs: None,
        }
    }

    pub fn ok_logs(logs: Vec<LogLine>) -> Self {
        Self::Ok {
            message: None,
            services: None,
            profile: None,
            logs: Some(logs),
        }
    }

    pub fn ok_services(services: Vec<ServiceStatus>) -> Self {
        Self::Ok {
            message: None,
            services: Some(services),
            profile: None,
            logs: None,
        }
    }

    pub fn ok_profile(profile: BootProfileReport) -> Self {
        Self::Ok {
            message: None,
            services: None,
            profile: Some(profile),
            logs: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}

pub fn encode_request(req: &ControlRequest) -> Result<String, serde_json::Error> {
    serde_json::to_string(req)
}

pub fn decode_request(line: &str) -> Result<ControlRequest, serde_json::Error> {
    serde_json::from_str(line.trim())
}

pub fn encode_response(resp: &ControlResponse) -> Result<String, serde_json::Error> {
    serde_json::to_string(resp)
}

pub fn decode_response(line: &str) -> Result<ControlResponse, serde_json::Error> {
    serde_json::from_str(line.trim())
}
