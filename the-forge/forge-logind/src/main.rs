use forge_common::DEFAULT_CONTROL_SOCKET;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const LOGIND_SOCKET: &str = "/run/forge/logind.sock";

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
enum LogindRequest {
    CreateSession { uid: u32 },
    ReleaseSession { uid: u32 },
    ListSessions,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum LogindResponse {
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        runtime_dir: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sessions: Option<Vec<SessionInfo>>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Serialize)]
struct SessionInfo {
    uid: u32,
    runtime_dir: String,
}

fn socket_path() -> String {
    std::env::var("FORGE_LOGIND_SOCKET").unwrap_or_else(|_| LOGIND_SOCKET.to_string())
}

fn runtime_dir_for(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/user/{uid}"))
}

fn ensure_runtime(uid: u32) -> Result<PathBuf, String> {
    let dir = runtime_dir_for(uid);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        // Chown the directory to the user so they can write to it
        let _ = std::process::Command::new("chown")
            .args([&format!("{uid}:{uid}"), &dir.display().to_string()])
            .status();
    }
    Ok(dir)
}

fn handle_client(stream: UnixStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
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

    let response = match serde_json::from_str::<LogindRequest>(&line) {
        Ok(LogindRequest::CreateSession { uid }) => match ensure_runtime(uid) {
            Ok(dir) => LogindResponse::Ok {
                runtime_dir: Some(dir.display().to_string()),
                sessions: None,
            },
            Err(e) => LogindResponse::Error { message: e },
        },
        Ok(LogindRequest::ReleaseSession { uid }) => {
            let dir = runtime_dir_for(uid);
            let _ = fs::remove_dir_all(dir);
            LogindResponse::Ok {
                runtime_dir: None,
                sessions: None,
            }
        }
        Ok(LogindRequest::ListSessions) => {
            let mut sessions = Vec::new();
            let base = Path::new("/run/user");
            if base.exists() {
                for entry in fs::read_dir(base).into_iter().flatten().flatten() {
                    if let Ok(uid) = entry.file_name().to_string_lossy().parse::<u32>() {
                        sessions.push(SessionInfo {
                            uid,
                            runtime_dir: entry.path().display().to_string(),
                        });
                    }
                }
            }
            LogindResponse::Ok {
                runtime_dir: None,
                sessions: Some(sessions),
            }
        }
        Err(e) => LogindResponse::Error {
            message: format!("invalid request: {e}"),
        },
    };

    if let Ok(payload) = serde_json::to_string(&response) {
        let _ = writeln!(writer, "{payload}");
    }
}

fn main() {
    let _ = fs::create_dir_all("/run/forge");
    let _ = fs::create_dir_all("/run/dbus");

    let _ = fs::create_dir_all("/run/user");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions("/run/user", fs::Permissions::from_mode(0o755));
    }

    let _ = fs::create_dir_all("/run/lock");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions("/run/lock", fs::Permissions::from_mode(0o1777));
    }

    let _ = ensure_runtime(0);

    let path = socket_path();
    if let Some(parent) = Path::new(&path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::remove_file(&path);

    let listener = UnixListener::bind(&path).unwrap_or_else(|e| {
        eprintln!("forge-logind: cannot bind {path}: {e}");
        std::process::exit(1);
    });

    eprintln!(
        "forge-logind: listening on {path} (control socket default: {DEFAULT_CONTROL_SOCKET})"
    );

    for client in listener.incoming().flatten() {
        handle_client(client);
    }
}
