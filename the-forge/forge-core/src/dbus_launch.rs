use std::path::Path;
use std::process::{Command, Stdio};

use crate::boot_debug;

const DBUS_DAEMON: &str = "/usr/bin/dbus-daemon";
const DBUS_BROKER_LAUNCH: &str = "/usr/bin/dbus-broker-launch";
const SETPRIV: &str = "/usr/bin/setpriv";
const EXEC_HELPER: &str = "/usr/libexec/forge/exec-selinux-service.sh";

fn tool_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

fn dbus_daemon_args() -> Vec<String> {
    let conf = if Path::new("/usr/share/dbus-1/system.conf").is_file() {
        "/usr/share/dbus-1/system.conf"
    } else {
        "/etc/dbus-1/system.conf"
    };
    vec![
        format!("--config-file={conf}"),
        "--nofork".into(),
        "--nopidfile".into(),
    ]
}

pub fn prepare_runtime() {
    let _ = std::fs::create_dir_all("/run/dbus");
    let _ = std::fs::create_dir_all("/var/lib/dbus");
    let _ = std::fs::remove_file("/run/dbus/system_bus_socket");
    let _ = std::fs::remove_file("/run/dbus/pid");
    if Path::new("/usr/bin/dbus-uuidgen").is_file() {
        let _ = Command::new("/usr/bin/dbus-uuidgen")
            .arg("--ensure=/var/lib/dbus/machine-id")
            .status();
    }
    let _ = Command::new("chown")
        .args(["root:root", "/run/dbus"])
        .status();
    let _ = Command::new("restorecon")
        .args(["-R", "/run/dbus"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("chcon")
        .args([
            "-u",
            "system_u",
            "-r",
            "object_r",
            "-t",
            "system_dbusd_var_run_t",
            "/run/dbus",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    boot_debug::log("dbus: prepared /run/dbus (stale socket removed, SELinux labels applied)");
}

struct BusLauncher {
    exec: &'static str,
    args: Vec<String>,
}

fn pick_launcher() -> Result<BusLauncher, String> {
    let listen_fds = std::env::var("LISTEN_FDS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let force_broker = std::env::var_os("FORGE_DBUS_USE_BROKER")
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true") || v == "yes");

    // dbus-broker-launch requires LISTEN_FDS from socket activation (systemd dbus.socket model).
    if listen_fds > 0 || force_broker {
        if tool_exists(DBUS_BROKER_LAUNCH) {
            boot_debug::log(&format!(
                "dbus: using dbus-broker-launch (LISTEN_FDS={listen_fds})"
            ));
            return Ok(BusLauncher {
                exec: DBUS_BROKER_LAUNCH,
                args: vec!["--scope".into(), "system".into(), "--audit".into()],
            });
        }
    }

    if listen_fds > 0 && tool_exists(DBUS_DAEMON) {
        let conf = if Path::new("/usr/share/dbus-1/system.conf").is_file() {
            "/usr/share/dbus-1/system.conf"
        } else {
            "/etc/dbus-1/system.conf"
        };
        boot_debug::log("dbus: using dbus-daemon --systemd-activation");
        return Ok(BusLauncher {
            exec: DBUS_DAEMON,
            args: vec![
                format!("--config-file={conf}"),
                "--nofork".into(),
                "--nopidfile".into(),
                "--systemd-activation".into(),
                "--address=systemd:".into(),
            ],
        });
    }

    if tool_exists(DBUS_DAEMON) {
        boot_debug::log("dbus: using dbus-daemon --nofork (standalone PID 1 bus)");
        return Ok(BusLauncher {
            exec: DBUS_DAEMON,
            args: dbus_daemon_args(),
        });
    }

    if tool_exists(DBUS_BROKER_LAUNCH) {
        boot_debug::log("dbus: fallback dbus-broker-launch (dbus-daemon missing)");
        return Ok(BusLauncher {
            exec: DBUS_BROKER_LAUNCH,
            args: vec!["--scope".into(), "system".into(), "--audit".into()],
        });
    }

    Err("neither dbus-daemon nor dbus-broker-launch found".into())
}

fn wrap_exec_helper(launcher: &BusLauncher) -> Command {
    boot_debug::log(&format!(
        "dbus: exec-selinux-service --user=dbus {} {}",
        launcher.exec,
        launcher.args.join(" ")
    ));
    let mut cmd = Command::new(EXEC_HELPER);
    cmd.arg("--user=dbus").arg(launcher.exec);
    cmd.args(&launcher.args);
    cmd
}

fn wrap_setpriv(launcher: &BusLauncher) -> Command {
    boot_debug::log(&format!("dbus: setpriv --reuid=dbus -- {}", launcher.exec));
    let mut cmd = Command::new(SETPRIV);
    cmd.args([
        "--reuid=dbus",
        "--regid=dbus",
        "--init-groups",
        "--",
        launcher.exec,
    ]);
    cmd.args(&launcher.args);
    cmd
}

/// Build the PID 1 system bus command.
pub fn system_dbus_command() -> Result<Command, String> {
    let launcher = pick_launcher()?;

    // init_t cannot runcon into system_dbusd_t — use exec domain transition (dbusd_exec_t).
    if tool_exists(EXEC_HELPER) {
        return Ok(wrap_exec_helper(&launcher));
    }

    if tool_exists(SETPRIV) {
        return Ok(wrap_setpriv(&launcher));
    }

    boot_debug::log(&format!(
        "dbus: exec {} (SELinux domain transition)",
        launcher.exec
    ));
    let mut cmd = Command::new(launcher.exec);
    cmd.args(&launcher.args);
    Ok(cmd)
}
