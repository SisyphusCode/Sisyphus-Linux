use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct SystemdUnit {
    pub sections: HashMap<String, HashMap<String, Vec<String>>>,
}

impl SystemdUnit {
    pub fn parse(content: &str) -> Self {
        let mut current = String::new();
        let mut sections: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                current = line[1..line.len() - 1].to_owned();
                sections.entry(current.clone()).or_default();
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                sections
                    .entry(current.clone())
                    .or_default()
                    .entry(key.trim().to_owned())
                    .or_default()
                    .push(value.trim().to_owned());
            }
        }

        Self { sections }
    }

    pub fn section(&self, name: &str) -> Option<&HashMap<String, Vec<String>>> {
        self.sections.get(name)
    }

    pub fn get(&self, section: &str, key: &str) -> Option<String> {
        self.section(section)?
            .get(key)
            .and_then(|values| values.first())
            .cloned()
    }

    pub fn values(&self, section: &str, key: &str) -> Vec<String> {
        self.section(section)
            .and_then(|sec| sec.get(key).cloned())
            .unwrap_or_default()
    }

    pub fn list(&self, section: &str, key: &str) -> Vec<String> {
        self.values(section, key)
            .into_iter()
            .flat_map(|value| {
                value
                    .split_whitespace()
                    .map(|part| normalize_unit_name(part).to_string())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

pub fn normalize_unit_name(name: &str) -> String {
    name.trim()
        .strip_suffix(".service")
        .or_else(|| name.strip_suffix(".socket"))
        .or_else(|| name.strip_suffix(".target"))
        .or_else(|| name.strip_suffix(".device"))
        .or_else(|| name.strip_suffix(".timer"))
        .or_else(|| name.strip_suffix(".mount"))
        .unwrap_or(name)
        .to_string()
}

pub fn unit_name_from_path(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(normalize_unit_name)
        .unwrap_or_else(|| "unknown".into())
}

pub fn parse_exec_start(line: &str) -> Result<(String, Vec<String>), String> {
    let parts = shell_words::split(line).map_err(|e| e.to_string())?;
    let exec = parts
        .first()
        .ok_or_else(|| "ExecStart is empty".to_string())?
        .clone();
    Ok((exec, parts.into_iter().skip(1).collect()))
}

pub fn map_restart(value: &str) -> super::forge::RestartPolicy {
    match value.to_ascii_lowercase().as_str() {
        "always" => super::forge::RestartPolicy::Always,
        "on-failure" | "on-abnormal" | "on-abort" | "on-watchdog" => {
            super::forge::RestartPolicy::OnFailure
        }
        _ => super::forge::RestartPolicy::No,
    }
}

pub fn map_service_type(value: &str) -> super::manifest::ServiceType {
    match value.to_ascii_lowercase().as_str() {
        "notify" => super::manifest::ServiceType::Notify,
        "notify-reload" => super::manifest::ServiceType::NotifyReload,
        "oneshot" => super::manifest::ServiceType::Oneshot,
        "dbus" => super::manifest::ServiceType::Dbus,
        "forking" => super::manifest::ServiceType::Forking,
        _ => super::manifest::ServiceType::Simple,
    }
}

pub fn collect_service_environment(unit: &SystemdUnit) -> Result<Vec<(String, String)>, String> {
    use crate::environment::{load_environment_file, parse_environment_line};
    use std::path::Path;

    let mut env = Vec::new();
    for file in unit.values("Service", "EnvironmentFile") {
        for path in file.split_whitespace() {
            env.extend(load_environment_file(Path::new(path))?);
        }
    }
    for line in unit.values("Service", "Environment") {
        if let Ok(parts) = shell_words::split(&line) {
            for part in parts {
                if let Some(pair) = parse_environment_line(&part) {
                    env.push(pair);
                }
            }
        } else {
            for part in line.split_whitespace() {
                if let Some(pair) = parse_environment_line(part) {
                    env.push(pair);
                }
            }
        }
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unit_sections() {
        let unit = SystemdUnit::parse(
            "[Unit]\nAfter=network.target\n\n[Service]\nExecStart=/bin/true\nEnvironment=FOO=bar\n",
        );
        assert_eq!(unit.get("Unit", "After"), Some("network.target".into()));
        assert_eq!(unit.get("Service", "ExecStart"), Some("/bin/true".into()));
        assert_eq!(unit.get("Service", "Environment"), Some("FOO=bar".into()));
    }

    #[test]
    fn strips_systemd_suffixes() {
        assert_eq!(normalize_unit_name("foo.service"), "foo");
        assert_eq!(normalize_unit_name("multi-user.target"), "multi-user");
    }
}
