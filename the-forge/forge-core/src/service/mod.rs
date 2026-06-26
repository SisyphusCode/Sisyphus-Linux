pub mod dropin;
pub mod forge;
pub mod manifest;
pub mod native;
pub mod socket;
pub mod systemd;
pub mod unit;

use std::sync::LazyLock;
use std::time::Instant;

static BOOT_CLOCK: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Real-time progressive boot telemetry with elapsed milliseconds.
pub fn ghosttype_log(status: &str, details: &str) {
    use std::io::Write;
    let elapsed = BOOT_CLOCK.elapsed().as_millis();
    let msg = format!("⏱️  [{:5}ms] ──┤ {:<10} ├── {}\n", elapsed, status, details);
    // Write to stdout (console)
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(msg.as_bytes());
    let _ = stdout.flush();
    // Also to kmsg for visibility in QEMU serial/console
    if let Ok(mut kmsg) = std::fs::File::create("/dev/kmsg") {
        let _ = kmsg.write_all(msg.as_bytes());
    }
}

pub fn boot_elapsed_ms() -> u128 {
    BOOT_CLOCK.elapsed().as_millis()
}
