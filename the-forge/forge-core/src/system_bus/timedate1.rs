use std::fs;

use zbus::blocking::Connection;
use zbus::interface;

pub fn register(conn: &Connection) -> zbus::Result<()> {
    conn.object_server()
        .at("/org/freedesktop/timedate1", Timedate1)?;
    Ok(())
}

struct Timedate1;

#[interface(name = "org.freedesktop.timedate1")]
impl Timedate1 {
    #[zbus(property)]
    async fn timezone(&self) -> zbus::fdo::Result<String> {
        Ok(read_timezone())
    }

    #[zbus(property)]
    async fn local_rtc(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    #[zbus(property)]
    async fn can_ntp(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    #[zbus(property)]
    async fn ntp(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    async fn set_timezone(&self, timezone: &str, _interactive: bool) -> zbus::fdo::Result<()> {
        let _ = fs::write("/etc/timezone", timezone);
        let _ = std::process::Command::new("timedatectl")
            .arg("set-timezone")
            .arg(timezone)
            .status();
        Ok(())
    }
}

fn read_timezone() -> String {
    fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| {
            p.to_string_lossy()
                .split("zoneinfo/")
                .nth(1)
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "UTC".into())
}
