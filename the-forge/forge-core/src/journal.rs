use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub ts: u128,
    pub unit: String,
    pub priority: u8,
    pub message: String,
    pub pid: Option<u32>,
}

fn journal_path() -> PathBuf {
    if let Ok(val) = std::env::var("FORGE_JOURNAL") {
        return PathBuf::from(val);
    }
    let primary = Path::new("/var/log/forge");
    if fs::create_dir_all(primary).is_ok() {
        let probe = primary.join(".journal-probe");
        if fs::write(&probe, b"").is_ok() {
            let _ = fs::remove_file(&probe);
            return primary.join("journal.jsonl");
        }
    }
    PathBuf::from("/run/forge/journal.jsonl")
}

use std::sync::atomic::{AtomicBool, Ordering};
static JOURNAL_PATH_INITIALIZED: AtomicBool = AtomicBool::new(false);
static JOURNAL_PATH_RESOLVED: LazyLock<Mutex<PathBuf>> =
    LazyLock::new(|| Mutex::new(PathBuf::from("/run/forge/journal.jsonl")));

pub fn get_journal_path() -> PathBuf {
    if let Ok(val) = std::env::var("FORGE_JOURNAL") {
        return PathBuf::from(val);
    }
    if JOURNAL_PATH_INITIALIZED.load(Ordering::Relaxed) {
        return JOURNAL_PATH_RESOLVED.lock().unwrap().clone();
    }
    let p = journal_path();
    if p.starts_with("/var/log") {
        *JOURNAL_PATH_RESOLVED.lock().unwrap() = p.clone();
        JOURNAL_PATH_INITIALIZED.store(true, Ordering::Relaxed);
    }
    p
}

static JOURNAL_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub fn init() -> Result<(), String> {
    let path = get_journal_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    spawn_journal_socket();
    Ok(())
}

/// Native journald-compatible datagram socket at /run/systemd/journal/socket.
fn spawn_journal_socket() {
    use std::os::unix::net::UnixDatagram;
    use std::thread;

    thread::spawn(|| {
        let path = "/run/systemd/journal/socket";
        let _ = fs::create_dir_all("/run/systemd/journal");
        let _ = fs::remove_file(path);
        let Ok(sock) = UnixDatagram::bind(path) else {
            return;
        };
        let mut buf = [0u8; 8192];
        loop {
            if let Ok(n) = sock.recv(&mut buf) {
                if n == 0 {
                    continue;
                }
                let msg = String::from_utf8_lossy(&buf[..n]);
                let line = msg.lines().next().unwrap_or(&msg);
                record("journald", 6, line.to_string(), None);
            }
        }
    });
}

pub fn record(unit: &str, priority: u8, message: impl Into<String>, pid: Option<u32>) {
    let entry = JournalEntry {
        ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        unit: unit.into(),
        priority,
        message: message.into(),
        pid,
    };

    if let Ok(line) = serde_json::to_string(&entry) {
        let _guard = JOURNAL_LOCK.lock();
        let path = get_journal_path();
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{line}");
        }
    }
}

pub fn read_unit_logs(unit: &str, tail: usize) -> Result<Vec<JournalEntry>, String> {
    let path = get_journal_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut entries: Vec<JournalEntry> = raw
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|entry: &JournalEntry| entry.unit == unit)
        .collect();

    if tail > 0 && entries.len() > tail {
        entries = entries.split_off(entries.len() - tail);
    }
    Ok(entries)
}

pub fn read_service_log_file(
    log_dir: &Path,
    unit: &str,
    tail: usize,
) -> Result<Vec<String>, String> {
    let path = log_dir.join(format!("{unit}.log"));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut lines: Vec<String> = raw.lines().map(str::to_string).collect();
    if tail > 0 && lines.len() > tail {
        lines = lines.split_off(lines.len() - tail);
    }
    Ok(lines)
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct JournalQuery {
    pub unit: Option<String>,
    pub priority_max: Option<u8>,
    pub since_ts: Option<u128>,
    pub tail: usize,
    pub follow: bool,
}

#[allow(dead_code)]
pub fn query_entries(q: &JournalQuery) -> Result<Vec<JournalEntry>, String> {
    let path = get_journal_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut entries: Vec<JournalEntry> = raw
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|entry: &JournalEntry| {
            if let Some(unit) = &q.unit {
                if &entry.unit != unit {
                    return false;
                }
            }
            if let Some(max_pri) = q.priority_max {
                if entry.priority > max_pri {
                    return false;
                }
            }
            if let Some(since) = q.since_ts {
                if entry.ts < since {
                    return false;
                }
            }
            true
        })
        .collect();

    if q.tail > 0 && entries.len() > q.tail {
        entries = entries.split_off(entries.len() - q.tail);
    }
    Ok(entries)
}

#[allow(dead_code)]
pub fn journal_path_display() -> PathBuf {
    get_journal_path()
}
