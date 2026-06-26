use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::service::ghosttype_log;

/// Native Plymouth teardown — replaces plymouth-forge-kill.sh.
pub fn spawn_quit_worker() {
    if std::process::id() != 1 {
        return;
    }
    thread::spawn(|| {
        let _ = std::fs::create_dir_all("/run/forge");
        let _ = std::fs::write("/run/forge/plymouth-disabled", b"1");
        ghosttype_log("PLYMOUTH", "Releasing VT/KMS from initramfs Plymouth");
        let end = std::time::Instant::now() + Duration::from_secs(120);
        while std::time::Instant::now() < end {
            if process_running("plymouthd") {
                kill_plymouth();
            }
            thread::sleep(Duration::from_secs(2));
        }
        ghosttype_log("PLYMOUTH", "Plymouth watchdog finished");
    });
}

#[allow(dead_code)]
pub fn quit_now() {
    kill_plymouth();
}

fn process_running(name: &str) -> bool {
    Command::new("pgrep")
        .arg("-x")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn kill_plymouth() {
    for sig in ["TERM", "TERM", "KILL"] {
        let _ = Command::new("pkill")
            .arg(format!("-{sig}"))
            .arg("plymouthd")
            .status();
        let _ = Command::new("pkill")
            .arg(format!("-{sig}"))
            .arg("plymouth")
            .status();
    }
    for cmd in ["plymouth quit", "plymouth deactivate"] {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.len() >= 2 {
            let _ = Command::new(parts[0]).args(&parts[1..]).status();
        }
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/var/log/forge/plymouth-kill.log")
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "plymouth quit invoked")
        });
}
