use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn strip_quotes(value: &str) -> String {
    let value = value.trim();
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        if value.len() >= 2 {
            value[1..value.len() - 1].to_string()
        } else {
            value.to_string()
        }
    } else {
        value.to_string()
    }
}

/// Parse `KEY=value` pairs from a systemd EnvironmentFile.
pub fn load_environment_file(path: &Path) -> Result<Vec<(String, String)>, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            out.push((key.trim().to_string(), strip_quotes(value)));
        }
    }
    Ok(out)
}

pub fn parse_environment_line(value: &str) -> Option<(String, String)> {
    let (key, val) = value.split_once('=')?;
    Some((key.trim().to_string(), strip_quotes(val)))
}

pub fn merge_environment(
    base: &[(String, String)],
    extra: &[(String, String)],
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (k, v) in base {
        map.insert(k.clone(), v.clone());
    }
    for (k, v) in extra {
        map.insert(k.clone(), v.clone());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_environment_line() {
        let (k, v) = parse_environment_line("FOO=bar baz").unwrap();
        assert_eq!(k, "FOO");
        assert_eq!(v, "bar baz");
    }

    #[test]
    fn parses_environment_quotes() {
        let (k, v) = parse_environment_line("FOO=\"bar baz\"").unwrap();
        assert_eq!(k, "FOO");
        assert_eq!(v, "bar baz");

        let (k, v) = parse_environment_line("FOO='bar'").unwrap();
        assert_eq!(k, "FOO");
        assert_eq!(v, "bar");
    }
}
