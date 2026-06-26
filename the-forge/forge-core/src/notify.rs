use std::fs;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::service::ghosttype_log;

pub fn notify_socket_dir() -> PathBuf {
    std::env::var("FORGE_NOTIFY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/forge/notify"))
}

pub fn prepare_notify_socket(service: &str) -> Result<(PathBuf, UnixDatagram), String> {
    let dir = notify_socket_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{service}.sock"));
    let _ = fs::remove_file(&path);

    let sock =
        UnixDatagram::bind(&path).map_err(|e| format!("notify bind {}: {e}", path.display()))?;
    Ok((path, sock))
}

/// Parsed notify message fields (subset of sd_notify protocol).
#[derive(Debug, Default, Clone)]
pub struct NotifyMessage {
    pub ready: bool,
    pub reloading: bool,
    pub stopping: bool,
    pub watchdog: bool,
    pub watchdog_usec: Option<u64>,
    pub status: Option<String>,
    pub raw: String,
}

pub fn parse_notify(msg: &str) -> NotifyMessage {
    let mut out = NotifyMessage {
        raw: msg.to_string(),
        ..Default::default()
    };
    for part in msg.split('\n') {
        let part = part.trim();
        if part.eq_ignore_ascii_case("READY=1") {
            out.ready = true;
        } else if part.eq_ignore_ascii_case("RELOADING=1") {
            out.reloading = true;
        } else if part.eq_ignore_ascii_case("STOPPING=1") {
            out.stopping = true;
        } else if part.eq_ignore_ascii_case("WATCHDOG=1") {
            out.watchdog = true;
        } else if let Some(v) = part.strip_prefix("WATCHDOG_USEC=") {
            if let Ok(us) = v.trim().parse::<u64>() {
                out.watchdog_usec = Some(us);
            }
        } else if let Some(v) = part.strip_prefix("STATUS=") {
            out.status = Some(v.trim().to_string());
        }
    }
    out
}

pub fn wait_for_ready(sock: &UnixDatagram, timeout: Duration) -> Result<bool, String> {
    // NOTE: This is polling (200ms timeout). Per plan/subagent, ideal future is
    // to register these per-service notify fds with the central epoll reactor
    // (reactor.rs) for true event-driven readiness + watchdog pings.
    // Would require associating fds back to UnitKey and handling in handle_ipc or new token.
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 256];

    while Instant::now() < deadline {
        sock.set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|e| e.to_string())?;
        match sock.recv(&mut buf) {
            Ok(n) => {
                let msg = String::from_utf8_lossy(&buf[..n]);
                let parsed = parse_notify(&msg);
                if parsed.ready {
                    return Ok(true);
                }
                if let Some(st) = &parsed.status {
                    ghosttype_log("NOTIFY", &format!("status update: {st}"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(format!("notify recv failed: {e}")),
        }
    }
    ghosttype_log("NOTIFY", "Timed out waiting for READY=1");
    Ok(false)
}

#[allow(dead_code)]
pub fn send_ready(path: &Path) -> Result<(), String> {
    let sock = UnixDatagram::unbound().map_err(|e| e.to_string())?;
    sock.connect(path).map_err(|e| e.to_string())?;
    sock.send(b"READY=1").map_err(|e| e.to_string())?;
    Ok(())
}
