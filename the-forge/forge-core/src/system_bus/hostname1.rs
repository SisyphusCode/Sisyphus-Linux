use std::fs;

use zbus::blocking::Connection;
use zbus::interface;

pub fn register(conn: &Connection) -> zbus::Result<()> {
    conn.object_server()
        .at("/org/freedesktop/hostname1", Hostname1)?;
    Ok(())
}

struct Hostname1;

#[interface(name = "org.freedesktop.hostname1")]
impl Hostname1 {
    #[zbus(property)]
    async fn hostname(&self) -> zbus::fdo::Result<String> {
        Ok(read_hostname())
    }

    #[zbus(property)]
    async fn static_hostname(&self) -> zbus::fdo::Result<String> {
        Ok(read_hostname())
    }

    #[zbus(property)]
    async fn pretty_hostname(&self) -> zbus::fdo::Result<String> {
        Ok(read_hostname())
    }

    async fn set_hostname(&self, hostname: &str, _interactive: bool) -> zbus::fdo::Result<()> {
        fs::write("/etc/hostname", format!("{hostname}\n"))
            .map_err(|e| zbus::fdo::Error::Failed(format!("set hostname: {e}")))?;
        let _ = std::process::Command::new("hostnamectl")
            .arg("set-hostname")
            .arg(hostname)
            .status();
        Ok(())
    }
}

fn read_hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "localhost".into())
}
