mod hostname1;
mod locale1;
mod systemd1;
mod timedate1;

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use zbus::blocking::Connection;

use crate::service::forge::ForgeState;
use crate::service::ghosttype_log;

static SYSTEM_BUS_STARTED: AtomicBool = AtomicBool::new(false);

/// Called once when the system dbus socket is ready (before logind needs systemd1).
pub fn ensure_running(state: Arc<Mutex<ForgeState>>) {
    if SYSTEM_BUS_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || {
        if let Err(e) = run_services(state) {
            ghosttype_log("DBUS", &format!("system bus services stopped: {e}"));
        }
    });
}

#[allow(dead_code)]
pub fn spawn_all(state: Arc<Mutex<ForgeState>>) {
    ensure_running(state);
}

fn run_services(state: Arc<Mutex<ForgeState>>) -> zbus::Result<()> {
    for attempt in 0..300 {
        if std::path::Path::new("/run/dbus/system_bus_socket").exists() {
            break;
        }
        if attempt == 299 {
            ghosttype_log(
                "DBUS",
                "system bus socket not ready — skipping Rust bus names",
            );
            return Ok(());
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }

    let conn = Connection::system()?;
    // Prefer the Python stub when shipped — it implements session scopes for logind/GDM.
    let python_stub = std::path::Path::new("/usr/libexec/forge/systemd1-stub.py").exists();
    let force_rust = std::env::var("FORGE_FORCE_RUST_SYSTEMD1").is_ok();
    if force_rust || !python_stub {
        conn.request_name("org.freedesktop.systemd1")?;
        systemd1::register(&conn, state.clone())?;
    } else {
        ghosttype_log(
            "DBUS",
            "deferring org.freedesktop.systemd1 to systemd1-stub (Python)",
        );
    }

    // Hostname/timedate/locale are optional names; ignore name-in-use errors.
    if conn.request_name("org.freedesktop.hostname1").is_ok() {
        hostname1::register(&conn)?;
    }
    if conn.request_name("org.freedesktop.timedate1").is_ok() {
        timedate1::register(&conn)?;
    }
    if conn.request_name("org.freedesktop.locale1").is_ok() {
        locale1::register(&conn)?;
    }

    ghosttype_log(
        "DBUS",
        "Rust system bus services online (systemd1, hostname1, timedate1, locale1)",
    );

    loop {
        thread::sleep(std::time::Duration::from_secs(3600));
    }
}
