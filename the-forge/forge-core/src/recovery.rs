use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::service::ghosttype_log;

const ATTEMPTS_PATH: &str = "/var/lib/forge/boot-attempts";
const BOOT_OK_PATH: &str = "/run/forge/boot-ok";
const MAX_BOOT_ATTEMPTS: u32 = 2;
const HANDOFF_DELAY: Duration = Duration::from_secs(90);

static HANDOFF_REQUESTED: AtomicBool = AtomicBool::new(false);

/// When false (default), Forge stays PID 1 on failure/shutdown instead of execing systemd.
pub fn recovery_handoff_enabled() -> bool {
    std::env::var("FORGE_RECOVERY_HANDOFF")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownAction {
    Reboot,
    Halt,
    PowerOff,
}

pub fn shutdown_action_from_env() -> ShutdownAction {
    match std::env::var("FORGE_SHUTDOWN_ACTION")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "halt" => ShutdownAction::Halt,
        "poweroff" | "power-off" => ShutdownAction::PowerOff,
        _ => ShutdownAction::Reboot,
    }
}

/// Final PID 1 step after services are stopped: sync filesystems and reboot/halt.
pub fn finish_system_shutdown(action: ShutdownAction) -> ! {
    ghosttype_log("SHUTDOWN", "Syncing filesystems...");
    let _ = Command::new("sync").status();
    unsafe {
        libc::sync();
    }

    let label = match action {
        ShutdownAction::Reboot => "reboot",
        ShutdownAction::Halt => "halt",
        ShutdownAction::PowerOff => "poweroff",
    };
    ghosttype_log("SHUTDOWN", &format!("Requesting system {label}"));

    #[cfg(target_os = "linux")]
    unsafe {
        let cmd = match action {
            ShutdownAction::Reboot => libc::RB_AUTOBOOT,
            ShutdownAction::Halt => libc::RB_HALT_SYSTEM,
            ShutdownAction::PowerOff => libc::RB_POWER_OFF,
        };
        libc::reboot(cmd);
    }

    emergency_forever();
}

/// Writable log directory: prefer /var/log/forge, fall back to tmpfs under /run.
pub fn resolve_log_dir() -> PathBuf {
    if process::id() != 1 {
        return PathBuf::from("/run/forge/log");
    }

    for dir in ["/var/log/forge", "/run/forge/log"] {
        let path = Path::new(dir);
        if fs::create_dir_all(path).is_err() {
            continue;
        }
        let probe = path.join(".write-probe");
        if fs::write(&probe, b"ok").is_ok() {
            let _ = fs::remove_file(&probe);
            if dir == "/run/forge/log" {
                ghosttype_log(
                    "RECOVERY",
                    "Using /run/forge/log for service logs (/var not writable)",
                );
            }
            return path.to_path_buf();
        }
    }
    PathBuf::from("/run/forge/log")
}

/// PID 1 entry: abort repeated failed forge boots and hand straight to systemd.
pub fn init_pid1_recovery() {
    install_panic_hook();

    let attempts = read_boot_attempts().saturating_add(1);
    write_boot_attempts(attempts);

    if attempts > MAX_BOOT_ATTEMPTS {
        ghosttype_log(
            "RECOVERY",
            &format!(
                "Forge failed {attempts} consecutive boots — entering rescue (handoff={})",
                recovery_handoff_enabled()
            ),
        );
        if recovery_handoff_enabled() {
            disable_forge_grub();
            handoff_to_systemd("too many consecutive failed forge boots");
        } else {
            std::env::set_var("FORGE_TARGET", "rescue");
            ghosttype_log("RECOVERY", "Forcing rescue target (handoff disabled)");
        }
    }

    ghosttype_log(
        "RECOVERY",
        &format!("Forge boot attempt {attempts}/{MAX_BOOT_ATTEMPTS}"),
    );
}

pub fn mark_boot_success() {
    let _ = fs::create_dir_all("/var/lib/forge");
    let _ = fs::write(ATTEMPTS_PATH, b"0");
    let _ = fs::create_dir_all("/run/forge");
    let _ = fs::write(BOOT_OK_PATH, b"1");
}

pub fn assess_boot_health(dbus_ok: bool, getty_ok: bool, critical_failed: bool) -> BootHealth {
    if dbus_ok && !critical_failed {
        return BootHealth::Healthy;
    }
    if getty_ok {
        return BootHealth::Degraded;
    }
    BootHealth::Failed
}

pub enum BootHealth {
    Healthy,
    Degraded,
    Failed,
}

pub fn post_boot_recovery(health: BootHealth) {
    match health {
        BootHealth::Healthy => {
            mark_boot_success();
            ghosttype_log("RECOVERY", "Boot healthy — reset attempt counter");
        }
        BootHealth::Degraded => {
            mark_boot_success();
            ghosttype_log(
                "RECOVERY",
                "Boot degraded (login available) — reset attempt counter; Ctrl+Alt+F2 for emergency shell",
            );
        }
        BootHealth::Failed => {
            spawn_emergency_getty();
            if recovery_handoff_enabled() {
                ghosttype_log(
                    "RECOVERY",
                    "Critical boot failure — emergency login; systemd handoff in 90s unless dbus recovers",
                );
                schedule_systemd_handoff(HANDOFF_DELAY, "critical services failed");
            } else {
                ghosttype_log(
                    "RECOVERY",
                    "Critical boot failure — emergency login on tty1/tty3 (handoff disabled)",
                );
            }
        }
    }
}

fn schedule_systemd_handoff(delay: Duration, reason: &'static str) {
    if !recovery_handoff_enabled() {
        return;
    }
    if HANDOFF_REQUESTED.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || {
        thread::sleep(delay);
        if Path::new(BOOT_OK_PATH).exists() {
            return;
        }
        handoff_to_systemd(reason);
    });
}

pub fn spawn_emergency_getty() {
    let agetty = resolve_agetty();
    let Some(agetty) = agetty else {
        ghosttype_log(
            "RECOVERY",
            "agetty not found — cannot spawn emergency login",
        );
        return;
    };

    for (tty, args) in [
        (
            "/dev/tty3",
            ["--noreset", "-a", "root", "-l", "/bin/bash", "tty3"],
        ),
        (
            "/dev/tty1",
            ["--noreset", "-a", "root", "-l", "/bin/bash", "tty1"],
        ),
    ] {
        if !Path::new(tty).exists() {
            continue;
        }
        match Command::new(&agetty)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => {
                ghosttype_log(
                    "RECOVERY",
                    &format!("Emergency root shell on {tty} (Ctrl+Alt+F1/F3)"),
                );
                return;
            }
            Err(e) => {
                ghosttype_log("RECOVERY", &format!("Emergency getty on {tty} failed: {e}"));
            }
        }
    }
}

fn resolve_agetty() -> Option<String> {
    for path in ["/usr/sbin/agetty", "/sbin/agetty"] {
        if Path::new(path).is_file() {
            return Some(path.to_string());
        }
    }
    Command::new("which")
        .arg("agetty")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn disable_forge_grub() {
    for script in [
        "/usr/sbin/forge-boot-disable",
        "/usr/bin/forge-boot-disable",
    ] {
        if !Path::new(script).is_executable() {
            continue;
        }
        match Command::new(script)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(s) if s.success() => {
                ghosttype_log(
                    "RECOVERY",
                    "Disabled forge as default boot (GRUB restored to systemd)",
                );
                return;
            }
            _ => {}
        }
    }
    ghosttype_log(
        "RECOVERY",
        "forge-boot-disable not installed — run: sudo forge-boot-disable",
    );
}

pub fn handoff_to_systemd(reason: &str) -> ! {
    if !recovery_handoff_enabled() {
        let _ = writeln!(
            std::io::stderr(),
            "⏱️  RECOVERY  ──┤ handoff disabled — staying up: {reason}"
        );
        spawn_emergency_getty();
        emergency_forever();
    }
    // Use fallible logging during handoff (stdout may be closed/broken pipe).
    let _ = writeln!(
        std::io::stderr(),
        "⏱️  RECOVERY  ──┤ handoff to systemd: {reason}"
    );
    let _ = std::fs::write(
        "/dev/kmsg",
        format!("forge RECOVERY: handoff {reason}\n").as_bytes(),
    );
    log_recovery(&format!("handoff: {reason}"));
    disable_forge_grub();

    for path in ["/usr/lib/systemd/systemd", "/lib/systemd/systemd"] {
        if Path::new(path).is_file() {
            let _ = std::fs::write(
                "/dev/kmsg",
                format!("forge: exec systemd {path}\n").as_bytes(),
            );
            let err = Command::new(path).arg("--system").exec();
            let _ = writeln!(
                std::io::stderr(),
                "forge RECOVERY exec {path} failed: {err}"
            );
        }
    }

    emergency_forever();
}

fn emergency_forever() -> ! {
    ghosttype_log(
        "RECOVERY",
        "systemd not found — emergency getty only; kernel panic if PID 1 exits",
    );
    spawn_emergency_getty();
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn read_boot_attempts() -> u32 {
    fs::read_to_string(ATTEMPTS_PATH)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn write_boot_attempts(n: u32) {
    let _ = fs::create_dir_all("/var/lib/forge");
    let _ = fs::write(ATTEMPTS_PATH, n.to_string());
    let _ = fs::write("/run/forge/boot-attempts", n.to_string());
}

fn log_recovery(msg: &str) {
    let _ = fs::create_dir_all("/var/log/forge");
    let _ = fs::create_dir_all("/run/forge");
    for path in ["/var/log/forge/recovery.log", "/run/forge/recovery.log"] {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{msg}");
        }
    }
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        log_recovery(&format!("panic: {info}"));
        disable_forge_grub();
        if recovery_handoff_enabled() {
            handoff_to_systemd("forge-core panicked");
        } else {
            spawn_emergency_getty();
            emergency_forever();
        }
    }));
}

trait PathExt {
    fn is_executable(&self) -> bool;
}

impl PathExt for Path {
    fn is_executable(&self) -> bool {
        fs::metadata(self)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

use std::os::unix::fs::PermissionsExt;
