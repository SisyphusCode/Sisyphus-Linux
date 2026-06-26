use super::manifest::ServiceManifest;
use super::systemd::SystemdUnit;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Merge systemd-style drop-in fragments from `<unit>.d/*.conf` directories.
pub fn apply_systemd_dropins(base_dir: &Path, unit_name: &str, unit: &mut SystemdUnit) {
    let drop_dir = base_dir.join(format!("{unit_name}.d"));
    if !drop_dir.exists() {
        return;
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(&drop_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("conf"))
        .collect();
    paths.sort();
    for path in paths {
        if let Ok(raw) = fs::read_to_string(&path) {
            merge_systemd_unit(unit, &SystemdUnit::parse(&raw));
        }
    }
}

fn merge_systemd_unit(base: &mut SystemdUnit, overlay: &SystemdUnit) {
    for (section, keys) in &overlay.sections {
        let entry = base.sections.entry(section.clone()).or_default();
        for (key, values) in keys {
            entry.insert(key.clone(), values.clone());
        }
    }
}

/// Merge TOML drop-in tables into a service manifest (environment vars, after, etc.).
pub fn apply_toml_dropins(unit_dir: &Path, unit_stem: &str, manifest: &mut ServiceManifest) {
    let drop_dir = unit_dir.join(format!("{unit_stem}.d"));
    if !drop_dir.exists() {
        return;
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(&drop_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext == "toml" || ext == "conf")
        })
        .collect();
    paths.sort();

    for path in paths {
        if path.extension().and_then(|e| e.to_str()) == Some("conf") {
            if let Ok(raw) = fs::read_to_string(&path) {
                let overlay = SystemdUnit::parse(&raw);
                if let Some(after) = overlay.get("Unit", "After") {
                    manifest
                        .after
                        .extend(after.split_whitespace().map(|s| s.to_string()));
                }
                for env_line in overlay.values("Service", "Environment") {
                    for part in env_line.split_whitespace() {
                        if let Some((k, v)) = part.split_once('=') {
                            manifest.environment.push((k.into(), v.into()));
                        }
                    }
                }
            }
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(table) = toml::from_str::<toml::Value>(&raw) {
                merge_toml_value(manifest, &table);
            }
        }
    }
}

fn merge_toml_value(manifest: &mut ServiceManifest, value: &toml::Value) {
    let Some(service) = value.get("service").and_then(|v| v.as_table()) else {
        return;
    };
    if let Some(after) = service.get("after").and_then(|v| v.as_array()) {
        for item in after {
            if let Some(s) = item.as_str() {
                manifest.after.push(s.to_string());
            }
        }
    }
    if let Some(env) = service.get("environment").and_then(|v| v.as_table()) {
        for (k, v) in env {
            if let Some(s) = v.as_str() {
                manifest.environment.push((k.clone(), s.to_string()));
            }
        }
    }
}

#[allow(dead_code)]
pub fn list_dropin_dirs(unit_dir: &Path, systemd_dir: &Path, unit_name: &str) -> Vec<PathBuf> {
    vec![
        unit_dir.join(format!("{unit_name}.forge.toml.d")),
        unit_dir.join(format!("{unit_name}.d")),
        systemd_dir.join(format!("{unit_name}.service.d")),
        systemd_dir.join(format!("{unit_name}.d")),
    ]
}

#[allow(dead_code)]
pub fn dropin_summary(dirs: &[PathBuf]) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for dir in dirs {
        if dir.exists() {
            let count = fs::read_dir(dir).map(|rd| rd.count()).unwrap_or(0);
            out.insert(dir.display().to_string(), count);
        }
    }
    out
}
