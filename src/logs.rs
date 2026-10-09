//! Bounded, read-only access to local telemetry, independent of AppKit.
use serde_json::Value;
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 12;
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 1000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Errors,
    Warnings,
    Operations,
}

#[derive(Clone)]
pub struct Entry {
    pub timestamp_ns: u128,
    pub record: Value,
}
impl Entry {
    pub fn is_operation(&self) -> bool {
        self.record["kind"] == "span"
    }
    pub fn is_error(&self) -> bool {
        self.record["severity"] == "ERROR"
            || self.record["severity"] == "FATAL"
            || (self.is_operation()
                && self.record["status"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("Error")))
    }
    pub fn matches(&self, filter: Filter, query: &str) -> bool {
        let matches = match filter {
            Filter::All => true,
            Filter::Errors => self.is_error(),
            Filter::Warnings => self.record["severity"] == "WARN",
            Filter::Operations => self.is_operation(),
        };
        matches
            && (query.trim().is_empty()
                || self
                    .details("")
                    .to_lowercase()
                    .contains(&query.trim().to_lowercase()))
    }
    pub fn details(&self, date: &str) -> String {
        let record = &self.record;
        let level = if self.is_operation() {
            if self.is_error() {
                "OPERAÇÃO · ERRO"
            } else {
                "OPERAÇÃO"
            }
        } else {
            match record["severity"].as_str().unwrap_or("INFO") {
                "ERROR" | "FATAL" => "ERRO",
                "WARN" => "AVISO",
                "DEBUG" | "TRACE" => "DEPURAÇÃO",
                _ => "INFORMAÇÃO",
            }
        };
        let name = record[if self.is_operation() { "name" } else { "body" }]
            .as_str()
            .unwrap_or("Evento");
        let mut text = format!("{date}   {level}\n{name}\n");
        if let Some(duration) = record["duration_ms"].as_f64() {
            text.push_str(&format!("Duração: {duration:.2} ms\n"));
        }
        if let Some(status) = record["status"].as_str() {
            text.push_str(&format!("Estado: {status}\n"));
        }
        if let Some(attributes) = record["attributes"].as_object() {
            for (key, value) in attributes {
                let value = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                text.push_str(&format!("{key}: {value}\n"));
            }
        }
        for (key, label) in [
            ("trace_id", "Trace"),
            ("span_id", "Span"),
            ("process.pid", "Processo"),
        ] {
            if !record[key].is_null() {
                let value = record[key]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| record[key].to_string());
                text.push_str(&format!("{label}: {value}\n"));
            }
        }
        text
    }
}

#[derive(Default)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    pub skipped: usize,
    pub limited: bool,
    pub issues: Vec<String>,
}

pub fn directories() -> io::Result<Vec<PathBuf>> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Pasta pessoal indisponível."))?;
    let base = PathBuf::from(home).join("Library/Logs");
    Ok(vec![base.join("RSX"), base.join("RXS")])
}

fn telemetry_name(name: &str) -> bool {
    (name.starts_with("rsx-") || name.starts_with("rxs-"))
        && (name.ends_with(".jsonl") || (1..=3).any(|i| name.ends_with(&format!(".jsonl.{i}"))))
}

fn read_tail(path: &Path) -> io::Result<(Vec<u8>, bool)> {
    // Do not follow a replaced telemetry file symlink.
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::other("O registro não é um arquivo regular."));
    }
    #[cfg(target_os = "macos")]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?
    };
    #[cfg(not(target_os = "macos"))]
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let offset = len.saturating_sub(MAX_READ_BYTES);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(MAX_READ_BYTES).read_to_end(&mut bytes)?;
    if offset > 0 {
        let start = bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |i| i + 1);
        bytes.drain(..start);
    }
    // The writer may still be appending its last record; show it on the next refresh.
    if let Some(end) = bytes.iter().rposition(|b| *b == b'\n') {
        bytes.truncate(end + 1);
    } else {
        bytes.clear();
    }
    Ok((bytes, offset > 0))
}

pub fn load(directories: &[PathBuf]) -> Snapshot {
    let mut snapshot = Snapshot::default();
    let mut files = Vec::new();
    for directory in directories {
        match fs::read_dir(directory) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) if telemetry_name(&entry.file_name().to_string_lossy()) => {
                            if let Ok(metadata) = fs::symlink_metadata(entry.path())
                                && metadata.is_file()
                            {
                                files.push((metadata.modified().ok(), entry.path()));
                            }
                        }
                        Ok(_) => {}
                        Err(e) => snapshot.issues.push(e.to_string()),
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => snapshot
                .issues
                .push(format!("{}: {e}", directory.display())),
        }
    }
    files.sort_by(|a, b| b.cmp(a));
    snapshot.limited = files.len() > MAX_FILES;
    for (_, path) in files.into_iter().take(MAX_FILES) {
        match read_tail(&path) {
            Ok((bytes, limited)) => {
                snapshot.limited |= limited;
                for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
                    let parsed = serde_json::from_slice::<Value>(line)
                        .ok()
                        .and_then(|record| {
                            let timestamp_key = match record["kind"].as_str()? {
                                "log" => "timestamp_unix_ns",
                                "span" => "end_unix_ns",
                                _ => return None,
                            };
                            let timestamp_ns: u128 =
                                record[timestamp_key].as_str()?.parse().ok()?;
                            if timestamp_ns > u64::MAX as u128 {
                                return None;
                            }
                            Some(Entry {
                                timestamp_ns,
                                record,
                            })
                        });
                    match parsed {
                        Some(entry) => snapshot.entries.push(entry),
                        None => snapshot.skipped += 1,
                    }
                    if snapshot.entries.len() >= MAX_ENTRIES * 2 {
                        snapshot
                            .entries
                            .sort_by_key(|entry| std::cmp::Reverse(entry.timestamp_ns));
                        snapshot.entries.truncate(MAX_ENTRIES);
                        snapshot.limited = true;
                    }
                }
            }
            Err(e) => snapshot.issues.push(format!("{}: {e}", path.display())),
        }
    }
    snapshot
        .entries
        .sort_by_key(|entry| std::cmp::Reverse(entry.timestamp_ns));
    snapshot.limited |= snapshot.entries.len() > MAX_ENTRIES;
    snapshot.entries.truncate(MAX_ENTRIES);
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn large_files_read_only_a_complete_recent_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rsx-large.jsonl");
        let mut bytes = vec![b'x'; MAX_READ_BYTES as usize + 100];
        bytes.push(b'\n');
        bytes.extend_from_slice(
            b"{\"kind\":\"log\",\"timestamp_unix_ns\":\"42\",\"body\":\"recent\"}\n",
        );
        fs::write(&path, bytes).unwrap();
        let (tail, limited) = read_tail(&path).unwrap();
        assert!(limited);
        assert!(tail.len() < 100);
        let snapshot = load(&[dir.path().to_owned()]);
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].timestamp_ns, 42);
        assert!(snapshot.limited);
    }
    #[test]
    fn reads_rotations_and_legacy_logs_newest_first_ignoring_partial_lines() {
        let dir = tempfile::tempdir().unwrap();
        let record = |n: u128| {
            json!({"kind":"log", "timestamp_unix_ns":n.to_string(), "body":"capture.succeeded", "severity":"INFO"}).to_string()
        };
        fs::write(
            dir.path().join("rsx-test.jsonl"),
            format!("{}\ninvalid\n{{\"kind\":", record(30)),
        )
        .unwrap();
        fs::write(
            dir.path().join("rxs-old.jsonl.1"),
            format!("{}\n", record(10)),
        )
        .unwrap();
        fs::write(dir.path().join("other.jsonl"), format!("{}\n", record(90))).unwrap();
        let snapshot = load(&[dir.path().to_owned(), dir.path().join("missing")]);
        assert_eq!(
            snapshot
                .entries
                .iter()
                .map(|e| e.timestamp_ns)
                .collect::<Vec<_>>(),
            [30, 10]
        );
        assert_eq!(snapshot.skipped, 1);
        assert!(snapshot.issues.is_empty());
    }
    #[test]
    fn filters_errors_and_searches_attributes_and_trace_ids() {
        let entry = Entry {
            timestamp_ns: 1,
            record: json!({"kind":"span", "name":"image.export", "status":"Error: synthetic failure", "trace_id":"abc123", "attributes":{"detail":"Permissão negada"}}),
        };
        assert!(entry.matches(Filter::Errors, "PERMISSÃO"));
        assert!(entry.matches(Filter::Operations, "abc123"));
        assert!(!entry.matches(Filter::Warnings, ""));
        assert!(!entry.matches(Filter::All, "absent"));
    }
    #[test]
    fn limits_history_and_ignores_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let lines = (0..1100)
            .map(|n| {
                json!({"kind":"log", "timestamp_unix_ns":n.to_string(), "body":"test"}).to_string()
                    + "\n"
            })
            .collect::<String>();
        fs::write(dir.path().join("rsx-test.jsonl"), &lines).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            dir.path().join("rsx-test.jsonl"),
            dir.path().join("rsx-link.jsonl"),
        )
        .unwrap();
        let snapshot = load(&[dir.path().to_owned()]);
        assert_eq!(snapshot.entries.len(), 1000);
        assert_eq!(snapshot.entries[0].timestamp_ns, 1099);
        assert!(snapshot.limited);
    }
}
