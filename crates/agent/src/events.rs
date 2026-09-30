use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use homeguard_common::wire::{EventEntry, EventKind};

/// Append-only JSONL journal of agent events, with monotonically increasing
/// sequence numbers. The app polls `/v1/events?since=N` so events survive
/// the app being closed when they happen.
pub struct EventLog {
    path: PathBuf,
    next_seq: Mutex<u64>,
}

impl EventLog {
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        let next_seq = match std::fs::File::open(&path) {
            Ok(file) => {
                let reader = std::io::BufReader::new(file);
                let mut last = 0u64;
                for line in reader.lines() {
                    // A torn last write (crash mid-append, no fsync) can
                    // leave invalid UTF-8 or a truncated line. Stop
                    // scanning rather than failing the whole open — losing
                    // sight of a couple of trailing events is fine; a
                    // startup crash loop with zero hosts protection is not.
                    let Ok(line) = line else { break };
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Ok(entry) = serde_json::from_str::<EventEntry>(&line) {
                        last = entry.seq;
                    }
                }
                last + 1
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 1,
            Err(e) => return Err(e),
        };
        Ok(EventLog {
            path,
            next_seq: Mutex::new(next_seq),
        })
    }

    pub fn append(&self, kind: EventKind, message: impl Into<String>) -> std::io::Result<EventEntry> {
        let mut next_seq = self.next_seq.lock().unwrap();
        let entry = EventEntry {
            seq: *next_seq,
            at: jiff::Timestamp::now().to_string(),
            kind,
            message: message.into(),
        };
        let line = serde_json::to_string(&entry).expect("EventEntry always serializes");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;
        *next_seq += 1;
        Ok(entry)
    }

    /// Events with `seq >= since`, plus the `next_seq` to pass back for the
    /// following page.
    pub fn since(&self, since: u64) -> std::io::Result<(Vec<EventEntry>, u64)> {
        let next_seq = *self.next_seq.lock().unwrap();
        let events = match std::fs::File::open(&self.path) {
            Ok(file) => {
                let reader = std::io::BufReader::new(file);
                reader
                    .lines()
                    .map_while(Result::ok)
                    .filter(|line| !line.trim().is_empty())
                    .filter_map(|line| serde_json::from_str::<EventEntry>(&line).ok())
                    .filter(|e| e.seq >= since)
                    .collect()
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok((events, next_seq))
    }
}

pub fn events_path_for_tests(dir: &Path) -> PathBuf {
    dir.join("events.jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_reads_back_events_with_increasing_seq() {
        let dir = tempfile::tempdir().unwrap();
        let log = EventLog::open(events_path_for_tests(dir.path())).unwrap();

        let e1 = log.append(EventKind::AgentStarted, "démarré").unwrap();
        let e2 = log.append(EventKind::PolicyApplied, "v1 appliquée").unwrap();
        assert_eq!(e1.seq, 1);
        assert_eq!(e2.seq, 2);

        let (events, next_seq) = log.since(1).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(next_seq, 3);

        let (events, _) = log.since(2).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "v1 appliquée");
    }

    #[test]
    fn resumes_sequence_numbering_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = events_path_for_tests(dir.path());
        {
            let log = EventLog::open(path.clone()).unwrap();
            log.append(EventKind::AgentStarted, "a").unwrap();
            log.append(EventKind::AgentStarted, "b").unwrap();
        }
        let log = EventLog::open(path).unwrap();
        let e3 = log.append(EventKind::AgentStarted, "c").unwrap();
        assert_eq!(e3.seq, 3);
    }

    #[test]
    fn open_survives_a_torn_last_line_instead_of_erroring() {
        let dir = tempfile::tempdir().unwrap();
        let path = events_path_for_tests(dir.path());
        {
            let log = EventLog::open(path.clone()).unwrap();
            log.append(EventKind::AgentStarted, "a").unwrap();
            log.append(EventKind::AgentStarted, "b").unwrap();
        }
        // Simulate a crash mid-append: invalid UTF-8 with no trailing
        // newline, appended after the two valid entries above.
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&[0xff, 0xfe, b'{', b'"']).unwrap();
        drop(file);

        let log = EventLog::open(path).expect("a corrupted tail must not fail open()");
        // The two valid entries were counted; the torn one was not.
        let next = log.append(EventKind::AgentStarted, "c").unwrap();
        assert_eq!(next.seq, 3);
    }
}
