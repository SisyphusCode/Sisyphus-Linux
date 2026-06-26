use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

#[derive(Debug, serde::Deserialize)]
struct JournalEntry {
    ts: u128,
    unit: String,
    priority: u8,
    message: String,
    pid: Option<u32>,
}

fn journal_path() -> PathBuf {
    if let Ok(val) = env::var("FORGE_JOURNAL") {
        return PathBuf::from(val);
    }
    let primary = PathBuf::from("/var/log/forge/journal.jsonl");
    if primary.exists() {
        return primary;
    }
    let fallback = PathBuf::from("/run/forge/journal.jsonl");
    if fallback.exists() {
        return fallback;
    }
    primary
}

fn usage() -> ! {
    eprintln!(
        "Usage: forge-journalctl [OPTIONS]\n\
         Options:\n\
           -u, --unit UNIT       Filter by unit name\n\
           -p, --priority N      Show entries with priority <= N\n\
           -n, --lines N         Show last N entries\n\
           --since-boot          Approximate last hour of entries\n\
         Environment: FORGE_JOURNAL"
    );
    process::exit(2);
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut unit: Option<String> = None;
    let mut priority_max: Option<u8> = None;
    let mut tail: usize = 0;
    let mut since_ts: Option<u128> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-u" | "--unit" => {
                i += 1;
                unit = Some(args.get(i).cloned().unwrap_or_else(|| usage()));
            }
            "-p" | "--priority" => {
                i += 1;
                priority_max = Some(
                    args.get(i)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| usage()),
                );
            }
            "-n" | "--lines" => {
                i += 1;
                tail = args
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            "--since-boot" => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                since_ts = Some(now.saturating_sub(3_600_000));
            }
            "-h" | "--help" => usage(),
            other => {
                eprintln!("forge-journalctl: unknown option '{other}'");
                usage();
            }
        }
        i += 1;
    }

    let path = journal_path();
    if !path.exists() {
        return;
    }

    let raw = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("forge-journalctl: {}: {e}", path.display());
        process::exit(1);
    });

    let mut entries: Vec<JournalEntry> = raw
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|entry: &JournalEntry| {
            if let Some(u) = &unit {
                if &entry.unit != u {
                    return false;
                }
            }
            if let Some(max_pri) = priority_max {
                if entry.priority > max_pri {
                    return false;
                }
            }
            if let Some(since) = since_ts {
                if entry.ts < since {
                    return false;
                }
            }
            true
        })
        .collect();

    if tail > 0 && entries.len() > tail {
        entries = entries.split_off(entries.len() - tail);
    }

    for entry in entries {
        let pid = entry.pid.map(|p| format!(" pid={p}")).unwrap_or_default();
        println!(
            "{} [{}] {}{pid}: {}",
            entry.ts, entry.priority, entry.unit, entry.message
        );
    }
}
