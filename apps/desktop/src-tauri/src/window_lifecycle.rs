//! Bounded local host diagnostics. Only fixed event names and numeric state are
//! recorded; navigation titles, source paths, accounts and credentials cannot
//! enter this log. No renderer or polling thread is needed to observe release.
use serde::Serialize;
use std::{
    collections::VecDeque, fs::OpenOptions, io::Write, path::PathBuf, sync::Mutex, time::Instant,
};

const MAX_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Started,
    Created,
    ShowRequested,
    Shown,
    Hidden,
    HideFailed,
    ReleaseRequested,
    ReleaseQueued,
    ReleaseFailed,
    Destroyed,
    Reopen,
    ExitRequested,
}

struct State {
    path: Option<PathBuf>,
    sequence: u64,
    started: Instant,
    recent: VecDeque<Record>,
}
pub struct Trace(Mutex<State>);
impl Default for Trace {
    fn default() -> Self {
        Self(Mutex::new(State {
            path: None,
            sequence: 0,
            started: Instant::now(),
            recent: VecDeque::new(),
        }))
    }
}

#[derive(Clone, Serialize)]
struct Record {
    utc: String,
    uptime_ms: u128,
    pid: u32,
    sequence: u64,
    event: Event,
    window_generation: u64,
    visible: Option<bool>,
}

#[derive(Serialize)]
pub struct Diagnostics {
    uptime_ms: u128,
    recent_events: Vec<Record>,
}

impl Trace {
    pub fn diagnostics(&self) -> Option<Diagnostics> {
        let state = self.0.lock().ok()?;
        Some(Diagnostics {
            uptime_ms: state.started.elapsed().as_millis(),
            recent_events: state.recent.iter().cloned().collect(),
        })
    }
    pub fn enable(&self, directory: PathBuf) {
        if std::fs::create_dir_all(&directory).is_ok()
            && let Ok(mut state) = self.0.lock()
        {
            state.path = Some(directory.join("window-host.jsonl"));
        }
    }

    pub fn record(&self, event: Event, generation: u64, visible: Option<bool>) {
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        let Some(path) = state.path.clone() else {
            return;
        };
        state.sequence = state.sequence.saturating_add(1);
        let record = Record {
            utc: chrono::Utc::now().to_rfc3339(),
            uptime_ms: state.started.elapsed().as_millis(),
            pid: std::process::id(),
            sequence: state.sequence,
            event,
            window_generation: generation,
            visible,
        };
        let Ok(mut line) = serde_json::to_vec(&record) else {
            return;
        };
        if state.recent.len() == 32 {
            state.recent.pop_front();
        }
        state.recent.push_back(record);
        line.push(b'\n');
        if std::fs::metadata(&path)
            .is_ok_and(|m| m.len().saturating_add(line.len() as u64) > MAX_BYTES)
            && std::fs::rename(&path, path.with_file_name("window-host.previous.jsonl")).is_err()
        {
            // A failed rotation must not turn bounded diagnostics into an unbounded log.
            return;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        if let Ok(mut file) = options.open(path) {
            let _ = file.write_all(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_rotate_with_a_fixed_size_and_fixed_field_set() {
        let directory = tempfile::tempdir().unwrap();
        let trace = Trace::default();
        trace.enable(directory.path().to_path_buf());
        for generation in 0..1000 {
            trace.record(Event::Created, generation, Some(false));
        }
        let diagnostics = serde_json::to_value(trace.diagnostics().unwrap()).unwrap();
        let events = diagnostics["recent_events"].as_array().unwrap();
        assert_eq!(events.len(), 32);
        assert_eq!(events[0]["sequence"], 969);
        assert_eq!(events[31]["sequence"], 1000);
        let paths = ["window-host.jsonl", "window-host.previous.jsonl"];
        for name in paths {
            let path = directory.path().join(name);
            assert!(path.metadata().unwrap().len() <= MAX_BYTES);
            for line in std::fs::read_to_string(path).unwrap().lines() {
                let record: serde_json::Value = serde_json::from_str(line).unwrap();
                let keys = record
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                assert_eq!(
                    keys,
                    [
                        "event",
                        "pid",
                        "sequence",
                        "uptime_ms",
                        "utc",
                        "visible",
                        "window_generation"
                    ]
                );
            }
        }
    }
}
