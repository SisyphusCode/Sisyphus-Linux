use std::fs;

use zbus::blocking::Connection;
use zbus::interface;

pub fn register(conn: &Connection) -> zbus::Result<()> {
    conn.object_server()
        .at("/org/freedesktop/locale1", Locale1)?;
    Ok(())
}

struct Locale1;

#[interface(name = "org.freedesktop.locale1")]
impl Locale1 {
    #[zbus(property)]
    async fn locale(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(read_locale())
    }

    async fn set_locale(&self, locale: Vec<String>, interactive: bool) -> zbus::fdo::Result<()> {
        let _ = interactive;
        let mut content = String::new();
        for entry in &locale {
            if entry.contains('=') {
                content.push_str(entry);
                content.push('\n');
            }
        }
        if !content.is_empty() {
            let _ = fs::write("/etc/locale.conf", content);
        }
        Ok(())
    }
}

fn read_locale() -> Vec<String> {
    let mut entries = Vec::new();
    if let Ok(raw) = fs::read_to_string("/etc/locale.conf") {
        for line in raw.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') && trimmed.contains('=') {
                entries.push(trimmed.to_string());
            }
        }
    }
    if entries.is_empty() {
        entries.push("LANG=en_US.UTF-8".into());
    }
    entries
}
