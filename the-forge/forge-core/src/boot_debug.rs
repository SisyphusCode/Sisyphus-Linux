use std::fs::OpenOptions;
use std::io::Write;

pub fn log(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    let _ = std::fs::create_dir_all("/var/log/forge");
    let _ = std::fs::create_dir_all("/var/lib/forge");
    for path in [
        "/var/log/forge/boot-debug.log",
        "/var/lib/forge/boot-debug.log",
    ] {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{msg}");
        }
    }
    crate::service::ghosttype_log("DEBUG", msg);
}

pub fn selinux_enforcing() -> bool {
    if let Ok(s) = std::fs::read_to_string("/sys/fs/selinux/enforce") {
        return s.trim() == "1";
    }
    std::fs::read_to_string("/etc/selinux/config")
        .ok()
        .is_some_and(|cfg| cfg.lines().any(|line| line.trim() == "SELINUX=enforcing"))
}

/// PID 1 must not use runcon for daemon domains — systemd relies on *_exec_t transitions.
#[allow(dead_code)]
pub fn should_use_runcon() -> bool {
    false
}
