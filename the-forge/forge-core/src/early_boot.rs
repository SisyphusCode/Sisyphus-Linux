//! Rust replacement for forge-early.sh + forge-run-layout.sh (PID 1 early boot).

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::service::ghosttype_log;

const RUN_DIRS: &[&str] = &[
    "/run/dbus",
    "/run/forge/log",
    "/run/user",
    "/run/lock",
    "/run/log",
    "/run/gdm",
    "/run/systemd/seats",
    "/run/systemd/sessions",
    "/run/systemd/users",
    "/run/systemd/inhibit",
    "/run/systemd/ask-password",
    "/run/systemd/machines",
    "/run/systemd/shutdown",
    "/run/NetworkManager",
    "/run/nvidia-persistenced",
    "/run/systemd/journal",
    "/run/systemd/resolve",
    "/run/udev",
    "/tmp/.X11-unix",
    "/var/log/forge",
    "/var/lib/dbus",
    "/var/tmp",
];

pub fn run() -> Result<(), String> {
    ghosttype_log("EARLY", "Running Rust early-boot setup");

    for dir in RUN_DIRS {
        let _ = fs::create_dir_all(dir);
    }

    let _ = fs::remove_file("/run/dbus/system_bus_socket");
    let _ = fs::remove_file("/run/dbus/pid");
    let _ = fs::remove_file("/run/nologin");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions("/run/dbus", fs::Permissions::from_mode(0o755));
        let _ = fs::set_permissions("/run/gdm", fs::Permissions::from_mode(0o0711));
        let _ = fs::set_permissions("/tmp/.X11-unix", fs::Permissions::from_mode(0o1777));
    }

    let _ = Command::new("chown")
        .args(["root:root", "/run/dbus"])
        .status();
    let _ = Command::new("chown")
        .args(["root:gdm", "/run/gdm"])
        .status();

    ensure_machine_id();
    apply_selinux_labels();
    kill_stale_initramfs_daemons();
    ensure_console_nodes();
    load_kernel_modules();
    setup_gdm_runtime_dir();
    setup_vt_acls();

    if Path::new("/usr/libexec/forge/release-graphics.sh").is_file() {
        let _ = Command::new("/usr/libexec/forge/release-graphics.sh").status();
    }

    ghosttype_log("EARLY", "Early-boot setup complete");
    Ok(())
}

fn ensure_machine_id() {
    if Path::new("/usr/bin/dbus-uuidgen").is_file() {
        let _ = Command::new("/usr/bin/dbus-uuidgen")
            .arg("--ensure=/var/lib/dbus/machine-id")
            .status();
    }
}

fn apply_selinux_labels() {
    if Path::new("/usr/libexec/forge/restorecon-forge.sh").is_file() {
        let _ = Command::new("/usr/libexec/forge/restorecon-forge.sh")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
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
    for dir in [
        "/run/systemd/seats",
        "/run/systemd/sessions",
        "/run/systemd/users",
    ] {
        let _ = Command::new("chcon")
            .args([
                "-u",
                "system_u",
                "-r",
                "object_r",
                "-t",
                "systemd_logind_var_run_t",
                dir,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn kill_stale_initramfs_daemons() {
    if std::env::var("FORGE_MOCK_BOOT").is_ok() {
        return;
    }
    if std::process::id() != 1 {
        return;
    }
    for sig in [
        "dbus-daemon",
        "dbus-broker",
        "systemd-logind",
        "elogind",
        "systemd-udevd",
        "udevd",
        "plymouthd",
        "plymouth",
    ] {
        let _ = Command::new("pkill").args(["-9", sig]).status();
    }
}

fn ensure_console_nodes() {
    for (dev, major, minor) in [
        ("/dev/tty0", 4, 0),
        ("/dev/tty1", 4, 1),
        ("/dev/tty2", 4, 2),
        ("/dev/tty3", 4, 3),
        ("/dev/console", 5, 1),
        ("/dev/null", 1, 3),
    ] {
        if !Path::new(dev).exists() {
            let _ = Command::new("mknod")
                .args([dev, "c", &major.to_string(), &minor.to_string()])
                .status();
            if dev.starts_with("/dev/tty") || dev == "/dev/console" {
                let _ = Command::new("chmod").args(["622", dev]).status();
            } else if dev == "/dev/null" {
                let _ = Command::new("chmod").args(["666", dev]).status();
            }
        }
    }
}

fn load_kernel_modules() {
    for module in [
        "i8042",
        "atkbd",
        "usbhid",
        "iwlwifi",
        "nvidia",
        "nvidia_drm",
        "nvidia_modeset",
        "drm",
    ] {
        let _ = Command::new("modprobe")
            .arg(module)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn setup_gdm_runtime_dir() {
    let output = Command::new("getent").args(["passwd", "gdm"]).output();
    if let Ok(out) = output {
        if out.status.success() {
            let line = String::from_utf8_lossy(&out.stdout);
            if let Some(uid) = line.split(':').nth(2) {
                let dir = format!("/run/user/{uid}");
                let _ = fs::create_dir_all(&dir);
                let _ = Command::new("chown")
                    .args([format!("gdm:gdm"), dir.clone()])
                    .status();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
                }
            }
        }
    }
}

fn setup_vt_acls() {
    let desktop_user = read_desktop_user();
    for vt in [
        "/dev/tty0",
        "/dev/tty1",
        "/dev/tty2",
        "/dev/tty3",
        "/dev/console",
    ] {
        if !Path::new(vt).exists() {
            continue;
        }
        let _ = Command::new("setfacl")
            .args(["-m", "u:gdm:rw,m::rw", vt])
            .status();
        let _ = Command::new("setfacl")
            .args(["-m", &format!("u:{desktop_user}:rw,m::rw"), vt])
            .status();
    }
}

fn read_desktop_user() -> String {
    let path = Path::new("/etc/forge/desktop.toml");
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("user") {
                if let Some((_, val)) = trimmed.split_once('=') {
                    let user = val.trim().trim_matches('"').trim_matches('\'').to_string();
                    if !user.is_empty() {
                        return user;
                    }
                }
            }
        }
    }
    "Sisyphus".to_string()
}
