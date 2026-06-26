use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;

use forge_common::{
    encode_response, ControlRequest, ControlResponse, LogLine, DEFAULT_CONTROL_SOCKET,
};

use crate::service::forge::ForgeState;

pub fn control_socket_path() -> String {
    std::env::var("FORGE_CONTROL_SOCKET").unwrap_or_else(|_| DEFAULT_CONTROL_SOCKET.to_string())
}

pub fn bind_control_socket() -> Result<UnixListener, String> {
    let path = control_socket_path();
    let path_obj = Path::new(&path);
    if let Some(parent) = path_obj.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&path);
    UnixListener::bind(&path).map_err(|e| format!("Cannot bind {path}: {e}"))
}

pub fn handle_client(state: &Arc<Mutex<ForgeState>>, stream: UnixStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone control socket"));
    let mut writer = stream;

    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .ok()
        .filter(|n| *n > 0)
        .is_none()
    {
        return;
    }

    let response = match forge_common::decode_request(&line) {
        Ok(req) => dispatch(state, req),
        Err(e) => ControlResponse::err(format!("invalid request: {e}")),
    };

    if let Ok(payload) = encode_response(&response) {
        let _ = writeln!(writer, "{payload}");
    }
}

fn dispatch(state: &Arc<Mutex<ForgeState>>, req: ControlRequest) -> ControlResponse {
    match req {
        ControlRequest::Status => {
            let state = state.lock();
            ControlResponse::ok_services(state.status_snapshot())
        }
        ControlRequest::BootProfile => {
            let state = state.lock();
            ControlResponse::ok_profile(state.boot_profile().clone())
        }
        ControlRequest::Logs { name, tail } => {
            let state = state.lock();
            let tail = tail.unwrap_or(50);
            let mut lines = Vec::new();

            for entry in crate::journal::read_unit_logs(&name, tail).unwrap_or_default() {
                lines.push(LogLine {
                    source: "journal".into(),
                    ts: Some(entry.ts),
                    message: entry.message,
                });
            }
            for line in crate::journal::read_service_log_file(state.log_dir(), &name, tail)
                .unwrap_or_default()
            {
                lines.push(LogLine {
                    source: "stdout".into(),
                    ts: None,
                    message: line,
                });
            }
            ControlResponse::ok_logs(lines)
        }
        ControlRequest::Start { name } => {
            let mut state = state.lock();
            match state.start_service_by_name(&name) {
                Ok(pid) => ControlResponse::ok_message(format!("started '{name}' (pid {pid})")),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::Stop { name } => {
            let mut state = state.lock();
            match state.stop_service_by_name(&name) {
                Ok(()) => ControlResponse::ok_message(format!("stopped '{name}'")),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::Restart { name } => {
            let mut state = state.lock();
            match state.restart_service_by_name(&name) {
                Ok(pid) => ControlResponse::ok_message(format!("restarted '{name}' (pid {pid})")),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::Reload { name } => {
            let mut state = state.lock();
            match state.reload_service_by_name(&name) {
                Ok(()) => ControlResponse::ok_message(format!("reloaded '{name}'")),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::ActivateTarget { name } => {
            let mut state = state.lock();
            match state.activate_target(&name) {
                Ok(()) => ControlResponse::ok_message(format!("target '{name}' activated")),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
        ControlRequest::Enable { service, runlevel } => {
            let runlevel = runlevel.unwrap_or_else(|| "graphical".to_string());
            match crate::rc_update::enable(&service, &runlevel) {
                Ok(()) => {
                    ControlResponse::ok_message(format!("enabled '{service}' for '{runlevel}'"))
                }
                Err(e) => ControlResponse::err(e),
            }
        }
        ControlRequest::Disable { service, runlevel } => {
            let runlevel = runlevel.unwrap_or_else(|| "graphical".to_string());
            match crate::rc_update::disable(&service, &runlevel) {
                Ok(()) => {
                    ControlResponse::ok_message(format!("disabled '{service}' for '{runlevel}'"))
                }
                Err(e) => ControlResponse::err(e),
            }
        }
        ControlRequest::Service { name } => {
            let state = state.lock();
            let services: Vec<_> = state
                .status_snapshot()
                .into_iter()
                .filter(|s| s.name == name)
                .collect();
            ControlResponse::ok_services(services)
        }
        ControlRequest::RcUpdateAdd { service, runlevel } => {
            match crate::rc_update::enable(&service, &runlevel) {
                Ok(()) => {
                    ControlResponse::ok_message(format!("enabled '{service}' for '{runlevel}'"))
                }
                Err(e) => ControlResponse::err(e),
            }
        }
        ControlRequest::RcUpdateDel { service, runlevel } => {
            match crate::rc_update::disable(&service, &runlevel) {
                Ok(()) => {
                    ControlResponse::ok_message(format!("disabled '{service}' for '{runlevel}'"))
                }
                Err(e) => ControlResponse::err(e),
            }
        }
        ControlRequest::RcUpdateShow { runlevel } => {
            let message = match runlevel {
                Some(rl) => match crate::rc_update::list_enabled(&rl) {
                    Ok(list) => format!("{rl}: {}", list.join(" ")),
                    Err(e) => return ControlResponse::err(e),
                },
                None => match crate::rc_update::list_all() {
                    Ok(all) => all
                        .into_iter()
                        .map(|(rl, svcs)| format!("{rl}: {}", svcs.join(" ")))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Err(e) => return ControlResponse::err(e),
                },
            };
            ControlResponse::ok_message(message)
        }
        ControlRequest::Shutdown => {
            let mut state = state.lock();
            match state.shutdown() {
                Ok(()) => ControlResponse::ok_message("shutdown initiated"),
                Err(e) => ControlResponse::err(e.to_string()),
            }
        }
    }
}
