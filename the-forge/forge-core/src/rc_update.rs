//! OpenRC-style runlevel enablement database (`rc-update` equivalent).
//!
//! Enabled services are marked with empty files under `/etc/forge/runlevels/<runlevel>/<service>`.
//!
//! Runlevel inheritance chain (each level inherits services from all levels to its right):
//!   graphical → multi-user → boot → sysinit
//!   single/rescue → sysinit  (parallel branch, not a parent of multi-user)

use std::fs;
use std::path::PathBuf;

use crate::service::ghosttype_log;

/// Map boot target name to OpenRC-style runlevel.
pub fn target_to_runlevel(target: &str) -> String {
    match target.to_lowercase().as_str() {
        "graphical" => "graphical".into(),
        "multi-user" => "multi-user".into(),
        "rescue" | "single" | "emergency" => "single".into(),
        "sysinit" => "sysinit".into(),
        "boot" => "boot".into(),
        _ => target.to_string(),
    }
}

/// Returns the ancestor chain for a runlevel, from most-specific to most-general.
///
/// A service enabled (via `rc-update add`) in any level in this chain is considered
/// enabled in `runlevel`. This mirrors OpenRC's stacked-runlevel inheritance.
pub fn runlevel_ancestors(runlevel: &str) -> &'static [&'static str] {
    match runlevel {
        "graphical" => &["graphical", "multi-user", "boot", "sysinit"],
        "multi-user" => &["multi-user", "boot", "sysinit"],
        "boot" => &["boot", "sysinit"],
        "sysinit" => &["sysinit"],
        "single" | "rescue" | "emergency" => &["single", "sysinit"],
        _ => &[],
    }
}

/// True when install has seeded `/etc/forge/runlevels/.seeded`.
pub fn is_database_active() -> bool {
    runlevels_root().join(".seeded").exists()
}

pub fn runlevels_root() -> PathBuf {
    std::env::var("FORGE_RUNLEVELS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/forge/runlevels"))
}

pub fn runlevel_dir(runlevel: &str) -> PathBuf {
    runlevels_root().join(runlevel)
}

pub fn marker_path(runlevel: &str, service: &str) -> PathBuf {
    runlevel_dir(runlevel).join(service)
}

fn any_marker_for_service(service: &str) -> bool {
    let root = runlevels_root();
    let Ok(entries) = fs::read_dir(&root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join(service).exists() {
            return true;
        }
    }
    false
}

/// Whether a unit's `runlevels` field allows it to run at the requested runlevel.
///
/// Respects inheritance: `runlevels = ["boot"]` makes the service eligible at
/// `multi-user` and `graphical` (because both inherit from `boot`).
pub fn unit_eligible_for_runlevel(unit_runlevels: &[String], runlevel: &str) -> bool {
    if unit_runlevels.is_empty() {
        return true;
    }
    // A service is eligible if any of its declared runlevels appears in the
    // ancestor chain of the current runlevel (i.e. the current level inherits it).
    let ancestors = runlevel_ancestors(runlevel);
    unit_runlevels
        .iter()
        .any(|rl| ancestors.contains(&rl.as_str()))
}

/// OpenRC `rc-update` — is this service enabled for the active runlevel?
///
/// Checks the marker database across the full ancestor chain so that a service
/// enabled in `boot` is automatically active in `multi-user` and `graphical`.
pub fn is_enabled(service: &str, runlevel: &str, unit_runlevels: &[String]) -> bool {
    if !unit_eligible_for_runlevel(unit_runlevels, runlevel) {
        return false;
    }
    let root = runlevels_root();
    if !root.exists() {
        return true;
    }
    // Check the runlevel and every ancestor (inheritance chain).
    for ancestor in runlevel_ancestors(runlevel) {
        if marker_path(ancestor, service).exists() {
            return true;
        }
    }
    // Seeded DB: services must be explicitly enabled somewhere in the chain.
    if root.join(".seeded").exists() {
        return false;
    }
    // Unseeded: enable if the service has never been registered in rc-update.
    !any_marker_for_service(service)
}

pub fn enable(service: &str, runlevel: &str) -> Result<(), String> {
    let dir = runlevel_dir(runlevel);
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(service);
    fs::write(&path, b"").map_err(|e| format!("enable {service}@{runlevel}: {e}"))?;
    ghosttype_log(
        "RC",
        &format!("Enabled '{service}' for runlevel '{runlevel}'"),
    );
    Ok(())
}

pub fn disable(service: &str, runlevel: &str) -> Result<(), String> {
    let path = marker_path(runlevel, service);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("disable {service}@{runlevel}: {e}"))?;
        ghosttype_log(
            "RC",
            &format!("Disabled '{service}' for runlevel '{runlevel}'"),
        );
    }
    Ok(())
}

pub fn list_enabled(runlevel: &str) -> Result<Vec<String>, String> {
    let dir = runlevel_dir(runlevel);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

pub fn list_all() -> Result<Vec<(String, Vec<String>)>, String> {
    let root = runlevels_root();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut levels: Vec<PathBuf> = fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    levels.sort();
    for level in levels {
        let name = level
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let services = list_enabled(&name)?;
        out.push((name, services));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Serialise all tests that mutate `FORGE_RUNLEVELS_DIR` so they don't race.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_runlevels() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("forge-rc-{stamp}"));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(".seeded"), b"1").unwrap();
        std::env::set_var("FORGE_RUNLEVELS_DIR", &path);
        path
    }

    #[test]
    fn enable_disable_roundtrip() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _root = temp_runlevels();
        enable("sshd", "multi-user").unwrap();
        assert!(is_enabled("sshd", "multi-user", &[]));
        // With inheritance, graphical inherits multi-user, so sshd IS enabled there too.
        assert!(is_enabled("sshd", "graphical", &[]));
        // boot is a PARENT of multi-user (more general), not a child — sshd NOT enabled in boot.
        assert!(!is_enabled("sshd", "boot", &[]));
        disable("sshd", "multi-user").unwrap();
        assert!(!is_enabled("sshd", "multi-user", &[]));
        let _ = std::env::remove_var("FORGE_RUNLEVELS_DIR");
    }

    #[test]
    fn runlevel_inheritance_chain() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _root = temp_runlevels();
        // Enable in boot — should propagate up to multi-user and graphical.
        enable("sshd", "boot").unwrap();
        assert!(is_enabled("sshd", "boot", &[]));
        assert!(is_enabled("sshd", "multi-user", &[]));
        assert!(is_enabled("sshd", "graphical", &[]));
        // single is a sibling branch — NOT inherited from boot.
        assert!(!is_enabled("sshd", "single", &[]));
        let _ = std::env::remove_var("FORGE_RUNLEVELS_DIR");
    }

    #[test]
    fn unit_runlevels_field_respects_inheritance() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _root = temp_runlevels();
        enable("sshd", "multi-user").unwrap();
        // unit_runlevels = ["multi-user"]: eligible for graphical (inherits multi-user)
        assert!(is_enabled("sshd", "graphical", &["multi-user".into()]));
        // unit_runlevels = ["multi-user"]: NOT eligible for boot (boot is less specific)
        assert!(!is_enabled("sshd", "boot", &["multi-user".into()]));
        let _ = std::env::remove_var("FORGE_RUNLEVELS_DIR");
    }
}
