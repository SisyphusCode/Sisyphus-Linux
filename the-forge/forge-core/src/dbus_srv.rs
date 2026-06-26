use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ipc;
use crate::service::forge::ForgeState;
use crate::service::ghosttype_log;

const FORGE_DBUS_NAME: &str = "org.forge1";
const SYSTEMD_DBUS_NAME: &str = "org.freedesktop.systemd1";

pub fn spawn_compat_server(state: Arc<Mutex<ForgeState>>) {
    std::thread::spawn(move || {
        if let Err(e) = run_socket_bridge(state) {
            ghosttype_log("DBUS", &format!("D-Bus bridge stopped: {e}"));
        }
    });
}

/// Lightweight compatibility layer: exposes Forge IPC over a second Unix socket
/// documented as a stand-in until full sd-bus/zbus system broker integration.
/// Clients use `FORGE_DBUS_SOCKET` with the same JSON protocol as forgectl.
fn run_socket_bridge(state: Arc<Mutex<ForgeState>>) -> Result<(), String> {
    use std::os::unix::net::UnixListener;

    let path = dbus_socket_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("dbus socket bind: {e}"))?;

    ghosttype_log(
        "DBUS",
        &format!(
            "Compat bridge online at {} (names: {FORGE_DBUS_NAME}, {SYSTEMD_DBUS_NAME})",
            path.display()
        ),
    );

    for stream in listener.incoming().flatten() {
        ipc::handle_client(&state, stream);
    }
    Ok(())
}

pub fn dbus_socket_path() -> PathBuf {
    std::env::var("FORGE_DBUS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/forge/dbus.sock"))
}

pub fn write_bus_info(out_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let socket = dbus_socket_path();
    let xml = format!(
        r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.forge1.Manager">
    <method name="StartUnit"><arg name="name" type="s" direction="in"/></method>
    <method name="StopUnit"><arg name="name" type="s" direction="in"/></method>
    <method name="ListUnits"/>
  </interface>
  <interface name="org.freedesktop.systemd1.Manager">
    <method name="StartUnit"><arg name="name" type="s" direction="in"/><arg name="mode" type="s" direction="in"/></method>
    <method name="StopUnit"><arg name="name" type="s" direction="in"/><arg name="mode" type="s" direction="in"/></method>
    <method name="ListUnits"/>
  </interface>
  <annotation name="forge.socket" value="{}"/>
</node>"#,
        socket.display()
    );
    std::fs::write(out_dir.join("forge-dbus.xml"), xml).map_err(|e| e.to_string())?;
    Ok(())
}
