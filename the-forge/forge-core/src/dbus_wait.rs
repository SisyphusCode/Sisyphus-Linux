use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::service::ghosttype_log;

fn dbus_reply_timeout_ms() -> u32 {
    std::env::var("FORGE_DBUS_REPLY_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500)
}

fn system_bus_address() -> String {
    std::env::var("DBUS_SYSTEM_BUS_ADDRESS")
        .unwrap_or_else(|_| "unix:path=/run/dbus/system_bus_socket".into())
}

fn resolve_tool(candidates: &[&str]) -> Option<PathBuf> {
    candidates
        .iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .map(|p| p.to_path_buf())
}

pub fn wait_for_bus_name(
    name: &str,
    timeout: Duration,
    child_pid: Option<u32>,
) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    let mut logged_wait = false;
    let is_core_bus = name == "org.freedesktop.DBus";

    while Instant::now() < deadline {
        if let Some(pid) = child_pid {
            let mut status: libc::c_int = 0;
            let result = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
            if result == pid as i32 {
                let detail = if libc::WIFEXITED(status) {
                    format!("exit code {}", libc::WEXITSTATUS(status))
                } else if libc::WIFSIGNALED(status) {
                    format!("signal {}", libc::WTERMSIG(status))
                } else {
                    "abnormal exit".into()
                };
                return Err(format!(
                    "service (pid {pid}) exited before D-Bus name '{name}' was acquired ({detail})"
                ));
            }
        }

        if is_core_bus {
            // Special case for the system bus itself: cannot query NameHasOwner reliably yet
            // (the daemon provides the bus that answers such queries). Just check that the
            // child is alive and the unix socket is present and connectable.
            if Path::new("/run/dbus/system_bus_socket").exists() {
                if let Ok(stream) =
                    std::os::unix::net::UnixStream::connect("/run/dbus/system_bus_socket")
                {
                    let _ = stream; // connect succeeded (fd is being listened on by the daemon)
                    ghosttype_log(
                        "DBUS",
                        &format!("Bus name '{name}' acquired (core bus socket responsive)"),
                    );
                    return Ok(true);
                }
                if !logged_wait {
                    ghosttype_log(
                        "DBUS",
                        &format!(
                            "Waiting for core bus socket to be responsive ({})",
                            system_bus_address()
                        ),
                    );
                    logged_wait = true;
                }
            }
        } else if name_has_owner(name)? {
            ghosttype_log("DBUS", &format!("Bus name '{name}' acquired"));
            return Ok(true);
        } else if !logged_wait && Path::new("/run/dbus/system_bus_socket").exists() {
            ghosttype_log(
                "DBUS",
                &format!("Waiting for '{name}' on {}", system_bus_address()),
            );
            logged_wait = true;
        }

        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(false)
}

fn name_has_owner(name: &str) -> Result<bool, String> {
    let addr = system_bus_address();

    if let Some(busctl) = resolve_tool(&["/usr/bin/busctl", "/bin/busctl"]) {
        let output = Command::new(&busctl)
            .env("DBUS_SYSTEM_BUS_ADDRESS", &addr)
            .args(["--timeout=1", "status", name])
            .output()
            .map_err(|e| format!("busctl: {e}"))?;
        return Ok(output.status.success());
    }

    let dbus_send = resolve_tool(&["/usr/bin/dbus-send", "/bin/dbus-send"])
        .ok_or_else(|| "dbus-send/busctl not found (PATH missing /usr/bin?)".to_string())?;

    let reply_timeout = dbus_reply_timeout_ms().to_string();
    let output = Command::new(&dbus_send)
        .env("DBUS_SYSTEM_BUS_ADDRESS", &addr)
        .args([
            &format!("--address={addr}"),
            &format!("--reply-timeout={reply_timeout}"),
            "--dest=org.freedesktop.DBus",
            "--print-reply",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.NameHasOwner",
            &format!("string:{name}"),
        ])
        .output()
        .map_err(|e| format!("dbus-send: {e}"))?;

    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.contains("boolean true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_absolute_tool_paths() {
        assert!(resolve_tool(&["/bin/sh"]).is_some());
    }
}
