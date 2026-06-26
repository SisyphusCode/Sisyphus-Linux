//! Rust service launchers — replace /usr/libexec/forge/start-*.sh wrappers.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

const EXEC_HELPER: &str = "/usr/libexec/forge/exec-selinux-service.sh";
const SETPRIV: &str = "/usr/bin/setpriv";

pub fn wrap_selinux(user: Option<&str>, binary: &str, args: &[String]) -> Command {
    if Path::new(EXEC_HELPER).is_file() {
        let mut cmd = Command::new(EXEC_HELPER);
        if let Some(u) = user {
            cmd.arg(format!("--user={u}"));
        }
        cmd.arg(binary);
        cmd.args(args);
        return cmd;
    }
    if let Some(u) = user {
        if Path::new(SETPRIV).is_file() {
            let mut cmd = Command::new(SETPRIV);
            cmd.args(["--reuid", u, "--regid", u, "--init-groups", "--", binary]);
            cmd.args(args);
            return cmd;
        }
    }
    let mut cmd = Command::new(binary);
    cmd.args(args);
    cmd
}

fn first_executable(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|p| Path::new(**p).is_file())
        .map(|s| (*s).to_string())
}

pub fn udevd_command(args: &[String]) -> Result<Command, String> {
    let bin = first_executable(&["/usr/lib/systemd/systemd-udevd", "/sbin/udevd"])
        .ok_or_else(|| "systemd-udevd not found".to_string())?;

    if std::process::id() == 1 && std::env::var("FORGE_MOCK_BOOT").is_err() {
        for sig in ["systemd-udevd", "udevd"] {
            let _ = Command::new("pkill").args(["-9", sig]).status();
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // logind (elogind or systemd-logind) pkill is handled in early_boot / vfs when needed.

    let mut cmd = Command::new(&bin);
    cmd.args(args);
    Ok(cmd)
}

pub fn logind_command(args: &[String]) -> Result<Command, String> {
    // Support both elogind (standalone for GUI on non-systemd) and systemd-logind.
    let bin = first_executable(&[
        "/usr/libexec/elogind/elogind",
        "/usr/lib/elogind/elogind",
        "/usr/lib64/elogind/elogind",
        "/usr/sbin/elogind",
        "/usr/bin/elogind",
        "/usr/lib/systemd/systemd-logind",
        "/lib/systemd/systemd-logind",
        "/usr/lib64/systemd/systemd-logind",
    ])
    .ok_or_else(|| "elogind or systemd-logind not found".to_string())?;

    wait_for_systemd1_stub()?;

    Ok(wrap_selinux(None, &bin, args))
}

pub fn polkit_command(args: &[String]) -> Result<Command, String> {
    let bin = first_executable(&["/usr/lib/polkit-1/polkitd", "/usr/libexec/polkitd"])
        .ok_or_else(|| "polkitd not found".to_string())?;

    let mut default_args = vec!["--no-debug".to_string(), "--log-level=err".to_string()];
    default_args.extend(args.iter().cloned());

    Ok(wrap_selinux(Some("polkitd"), &bin, &default_args))
}

pub fn accounts_daemon_command(args: &[String]) -> Result<Command, String> {
    let bin = first_executable(&["/usr/libexec/accounts-daemon", "/usr/lib/accounts-daemon"])
        .ok_or_else(|| "accounts-daemon not found".to_string())?;
    Ok(wrap_selinux(None, &bin, args))
}

pub fn network_manager_command(args: &[String]) -> Result<Command, String> {
    let bin = first_executable(&["/usr/sbin/NetworkManager", "/usr/bin/NetworkManager"])
        .ok_or_else(|| "NetworkManager not found".to_string())?;
    Ok(wrap_selinux(None, &bin, args))
}

pub fn agetty_command(args: &[String]) -> Result<Command, String> {
    let bin = first_executable(&["/usr/sbin/agetty", "/sbin/agetty"])
        .ok_or_else(|| "agetty not found".to_string())?;
    let mut cmd = Command::new(bin);
    cmd.args(args);
    Ok(cmd)
}

fn wait_for_systemd1_stub() -> Result<(), String> {
    let bus = std::env::var("DBUS_SYSTEM_BUS_ADDRESS")
        .unwrap_or_else(|_| "unix:path=/run/dbus/system_bus_socket".into());
    for _ in 0..150 {
        if Path::new("/usr/bin/busctl").is_file() {
            if Command::new("busctl")
                .args(["--address", &bus, "status", "org.freedesktop.systemd1"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                return Ok(());
            }
        } else if Path::new("/run/dbus/system_bus_socket").exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("org.freedesktop.systemd1 not ready on system bus".into())
}

/// Resolve a native service name to a Rust-built Command when the shell wrapper is obsolete.
pub fn resolve_service_command(
    name: &str,
    exec: &str,
    args: &[String],
) -> Option<Result<Command, String>> {
    match name {
        "forge-early" => None, // handled inline in forge.rs
        "dbus" if exec.contains("start-dbus") || exec.contains("dbus-daemon") => {
            Some(crate::dbus_launch::system_dbus_command())
        }
        "udev" if exec.contains("start-udevd") => Some(udevd_command(args)),
        "logind"
            if exec.contains("start-logind")
                || exec.contains("elogind")
                || exec.contains("systemd-logind") =>
        {
            Some(logind_command(args))
        }
        "polkit" if exec.contains("start-polkit") => Some(polkit_command(args)),
        "accounts-daemon" if exec.contains("start-accounts-daemon") => {
            Some(accounts_daemon_command(args))
        }
        "NetworkManager" if exec.contains("start-networkmanager") => {
            Some(network_manager_command(args))
        }
        n if n.starts_with("getty") && exec.contains("start-agetty") => Some(agetty_command(args)),
        _ => None,
    }
}
