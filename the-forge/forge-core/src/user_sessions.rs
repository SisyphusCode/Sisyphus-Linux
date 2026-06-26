use std::fs;
use std::path::Path;

use crate::service::ghosttype_log;

/// Native equivalent of systemd-user-sessions.service start.
pub fn allow_logins() -> Result<(), String> {
    let nologin = Path::new("/run/nologin");
    if nologin.exists() {
        fs::remove_file(nologin).map_err(|e| e.to_string())?;
        ghosttype_log("SESSIONS", "Removed /run/nologin — user logins enabled");
    } else {
        ghosttype_log(
            "SESSIONS",
            "No /run/nologin present — logins already allowed",
        );
    }
    Ok(())
}

/// Block logins (systemd-user-sessions stop).
#[allow(dead_code)]
pub fn block_logins(message: &str) -> Result<(), String> {
    fs::write("/run/nologin", format!("{message}\n")).map_err(|e| e.to_string())?;
    ghosttype_log("SESSIONS", "Created /run/nologin");
    Ok(())
}
