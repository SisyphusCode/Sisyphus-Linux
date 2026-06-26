use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ServiceLimits {
    #[serde(default, rename = "memory-max")]
    pub memory_max: Option<String>,
    #[serde(default, rename = "tasks-max")]
    pub tasks_max: Option<u64>,
}

const FORGE_ROOT: &str = "/sys/fs/cgroup/forge.slice";

pub fn root_path() -> PathBuf {
    std::env::var("FORGE_CGROUP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(FORGE_ROOT))
}

pub fn prepare_hierarchy() -> Result<(), String> {
    let root = root_path();
    if !Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        return Err("cgroup v2 unified hierarchy not mounted".into());
    }

    fs::create_dir_all(&root).map_err(|e| e.to_string())?;

    // Delegate controllers into our slice when running as PID 1 or root.
    let _ = enable_controllers(
        Path::new("/sys/fs/cgroup"),
        &["memory", "pids", "cpu", "io"],
    );
    let _ = enable_controllers(&root, &["memory", "pids", "cpu", "io"]);

    Ok(())
}

fn enable_controllers(path: &Path, controllers: &[&str]) -> Result<(), String> {
    let file = path.join("cgroup.subtree_control");
    if !file.exists() {
        return Ok(());
    }
    let mut spec = String::new();
    for ctrl in controllers {
        if !spec.is_empty() {
            spec.push(' ');
        }
        spec.push_str(&format!("+{ctrl}"));
    }
    if !spec.is_empty() {
        fs::write(&file, &spec).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn attach_service(name: &str, pid: u32, limits: &ServiceLimits) -> Result<(), String> {
    let root = root_path();
    if !root.exists() {
        return Ok(());
    }

    let dir = root.join(sanitize_name(name));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    if let Some(max) = &limits.memory_max {
        let _ = fs::write(dir.join("memory.max"), normalize_memory_max(max));
    }
    if let Some(max) = &limits.tasks_max {
        let _ = fs::write(dir.join("pids.max"), max.to_string());
    }

    fs::write(dir.join("cgroup.procs"), pid.to_string()).map_err(|e| e.to_string())
}

fn sanitize_name(name: &str) -> String {
    name.replace('/', "_")
}

fn normalize_memory_max(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("max") || trimmed.eq_ignore_ascii_case("infinity") {
        return "max".into();
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return trimmed.to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    let num_part = lower.strip_suffix('b').unwrap_or(&lower);

    if let Some(num) = num_part.strip_suffix('k') {
        return (num.trim().parse::<u64>().unwrap_or(0) * 1024).to_string();
    }
    if let Some(num) = num_part.strip_suffix('m') {
        return (num.trim().parse::<u64>().unwrap_or(0) * 1024 * 1024).to_string();
    }
    if let Some(num) = num_part.strip_suffix('g') {
        return (num.trim().parse::<u64>().unwrap_or(0) * 1024 * 1024 * 1024).to_string();
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_suffixes_convert_to_bytes() {
        assert_eq!(
            normalize_memory_max("512M"),
            (512 * 1024 * 1024).to_string()
        );
        assert_eq!(
            normalize_memory_max("512Mb"),
            (512 * 1024 * 1024).to_string()
        );
        assert_eq!(
            normalize_memory_max("2G"),
            (2u64 * 1024 * 1024 * 1024).to_string()
        );
        assert_eq!(
            normalize_memory_max("2gb"),
            (2u64 * 1024 * 1024 * 1024).to_string()
        );
        assert_eq!(normalize_memory_max("max"), "max");
    }
}
