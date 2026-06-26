use std::path::Path;
use std::process::{Command, Stdio};

use crate::service::ghosttype_log;

/// Relabel paths systemd would create via mkdir-label / restorecon.
pub fn relabel_paths(paths: &[&str]) {
    if !Path::new("/sys/fs/selinux").is_dir() {
        return;
    }

    let restorecon = if Path::new("/sbin/restorecon").is_file() {
        "/sbin/restorecon"
    } else if Path::new("/usr/sbin/restorecon").is_file() {
        "/usr/sbin/restorecon"
    } else {
        return;
    };

    for path in paths {
        if !Path::new(path).exists() {
            continue;
        }
        let _ = Command::new(restorecon)
            .args(["-F", path])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Relabel runtime trees logind, dbus, and NetworkManager expect.
pub fn relabel_runtime_trees() {
    if Path::new("/usr/libexec/forge/restorecon-forge.sh").is_file() {
        let _ = Command::new("/usr/libexec/forge/restorecon-forge.sh")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return;
    }

    for dir in [
        "/run/dbus",
        "/run/systemd",
        "/run/gdm",
        "/run/NetworkManager",
        "/run/user",
        "/tmp/.X11-unix",
    ] {
        relabel_tree(dir);
    }
}

fn relabel_tree(dir: &str) {
    if !Path::new(dir).is_dir() {
        return;
    }
    let restorecon = if Path::new("/sbin/restorecon").is_file() {
        "/sbin/restorecon"
    } else if Path::new("/usr/sbin/restorecon").is_file() {
        "/usr/sbin/restorecon"
    } else {
        return;
    };
    let _ = Command::new(restorecon)
        .args(["-R", dir])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Transition PID 1 from kernel_t to init_t using the exe's file context (systemd selinux-setup.c).
pub fn transition_to_init_domain() -> bool {
    if std::process::id() != 1 || !Path::new("/sys/fs/selinux").is_dir() {
        return false;
    }

    let current = std::fs::read_to_string("/proc/self/attr/current")
        .unwrap_or_default()
        .trim()
        .to_string();
    if !current.contains("kernel") {
        ghosttype_log(
            "VFS",
            &format!("SELinux domain '{current}' — init transition not required"),
        );
        return false;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };

    if let Some(label) = compute_init_context(&exe) {
        if apply_context(&label) {
            ghosttype_log("VFS", &format!("SELinux transitioned PID 1 to '{label}'"));
            return true;
        }
    }

    false
}

fn compute_init_context(exe: &Path) -> Option<String> {
    // Prefer setfiles/matchpathcon + security_compute_create via setcon wrapper.
    if Path::new("/usr/sbin/setcon").is_file() {
        // init_exec_t transitions to init_t on exec; setcon applies the target directly.
        return Some("system_u:system_r:init_t:s0".into());
    }

    let output = Command::new("matchpathcon").arg(exe).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let ctx = String::from_utf8_lossy(&output.stdout);
    let file_ctx = ctx.split_whitespace().next()?.trim();
    if file_ctx.is_empty() {
        return None;
    }
    // File context init_exec_t → process context init_t is policy-defined; use setcon target.
    Some("system_u:system_r:init_t:s0".into())
}

fn apply_context(label: &str) -> bool {
    if Path::new("/usr/sbin/setcon").is_file() {
        return Command::new("/usr/sbin/setcon")
            .arg(label)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }

    std::fs::write("/proc/self/attr/current", format!("{label}\0"))
        .or_else(|_| std::fs::write("/proc/self/attr/exec", format!("{label}\0")))
        .is_ok()
}
