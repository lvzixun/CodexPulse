//! FSEvents may not report appends while Codex keeps a rollout open. Probe only
//! the newest local rollouts; the existing collector still owns reads/cursors.
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const LIMIT: usize = 128;
const INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    modified: SystemTime,
    len: u64,
    identity: (u64, u64),
}
impl Stamp {
    fn read(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        meta.is_file().then_some(Self {
            modified: meta.modified().ok()?,
            len: meta.len(),
            identity: (meta.dev(), meta.ino()),
        })
    }
}

pub struct LiveFiles {
    files: BTreeMap<(String, PathBuf), Stamp>,
    last_poll: Instant,
}
impl Default for LiveFiles {
    fn default() -> Self {
        Self {
            files: BTreeMap::new(),
            last_poll: Instant::now(),
        }
    }
}
impl LiveFiles {
    /// Called immediately before a normal read, never after it: a write racing
    /// that read must remain detectable on the next probe.
    pub fn track(&mut self, source: &str, path: &Path) {
        let Some(stamp) = Stamp::read(path) else {
            return;
        };
        let key = (source.to_owned(), path.to_owned());
        if !self.files.contains_key(&key) && self.files.len() >= LIMIT {
            let (oldest, previous) = self
                .files
                .iter()
                .min_by_key(|(_, stamp)| stamp.modified)
                .expect("nonempty bounded set");
            if stamp.modified <= previous.modified {
                return;
            }
            let oldest = oldest.clone();
            self.files.remove(&oldest);
        }
        self.files.insert(key, stamp);
    }

    pub fn changed(
        &mut self,
        now: Instant,
        eligible: impl Fn(&str, &Path) -> bool,
    ) -> Vec<(String, PathBuf)> {
        if now.saturating_duration_since(self.last_poll) < INTERVAL {
            return Vec::new();
        }
        self.last_poll = now;
        let mut changed = Vec::new();
        self.files.retain(|(source, path), previous| {
            // Check source ownership before touching the filesystem. Disabled,
            // removed and remapped sources cannot be read by this fallback.
            if !eligible(source, path) {
                return false;
            }
            let Some(stamp) = Stamp::read(path) else {
                return false;
            };
            if stamp != *previous {
                *previous = stamp;
                changed.push((source.clone(), path.clone()));
            }
            true
        });
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulse_core::{collectors::jsonl::read_batch, pricing::PriceBook, storage::Store};
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn open_writer_updates_model_and_speed_without_events_or_duplicate_usage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let mut writer = fs::File::create(&path).unwrap();
        writeln!(writer, "{}", json!({"type":"session_meta","timestamp":"2026-11-15T00:00:00Z","payload":{"id":"live"}})).unwrap();
        let mut files = LiveFiles::default();
        let mut store = Store::in_memory().unwrap();
        let read = |store: &mut Store| {
            read_batch(store, "macos", &path, chrono_tz::UTC, &PriceBook::default()).unwrap()
        };
        files.track("macos", &path);
        read(&mut store);
        assert!(
            store
                .session_by_id("live")
                .unwrap()
                .unwrap()
                .meta
                .current_model
                .is_none()
        );
        for record in [
            json!({"type":"event_msg","timestamp":"2026-11-15T00:00:00Z","payload":{"type":"task_started","turn_id":"turn"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn","model":"gpt-6.1-sol","effort":"xhigh"}}),
            json!({"type":"event_msg","timestamp":"2026-11-15T00:00:10Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":20,"total_tokens":120}}}}),
        ] {
            writeln!(writer, "{record}").unwrap();
        }
        let now = Instant::now();
        assert!(files.changed(now, |_, _| true).is_empty());
        assert_eq!(
            files.changed(now + INTERVAL, |_, _| true),
            vec![("macos".into(), path.clone())]
        );
        files.track("macos", &path);
        assert_eq!(read(&mut store).inserted, 1);
        let session = store.session_by_id("live").unwrap().unwrap();
        assert_eq!(session.meta.current_model.as_deref(), Some("gpt-6.1-sol"));
        let rate = session.meta.output_rate.unwrap();
        assert_eq!((rate.output_tokens, rate.elapsed_ms), (20, 10_000));
        assert!(files.changed(now + INTERVAL * 2, |_, _| true).is_empty());
        assert!(read(&mut store).unchanged);
        assert_eq!(store.session_by_id("live").unwrap().unwrap().total, 120);

        // A partial append is retried only after more bytes arrive.
        writer.write_all(b"{\"type\":\"event_msg\"").unwrap();
        assert_eq!(files.changed(now + INTERVAL * 3, |_, _| true).len(), 1);
        files.track("macos", &path);
        assert_eq!(read(&mut store).records, 0);
        assert!(files.changed(now + INTERVAL * 4, |_, _| true).is_empty());
        writer.write_all(b",\"timestamp\":\"2026-11-15T00:00:11Z\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn\"}}\n").unwrap();
        assert_eq!(files.changed(now + INTERVAL * 5, |_, _| true).len(), 1);
        files.track("macos", &path);
        read(&mut store);
        assert_eq!(
            store.session_by_id("live").unwrap().unwrap().meta.status,
            "completed"
        );
        // Keep the writer alive through the full regression.
        writer.flush().unwrap();
    }

    #[test]
    fn disabled_sources_and_removed_files_are_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        fs::write(&path, "old").unwrap();
        let mut files = LiveFiles::default();
        files.track("removed", &path);
        files.track("local", &path);
        fs::remove_file(&path).unwrap();
        // Source filtering must run even when the path no longer exists.
        let now = Instant::now() + INTERVAL;
        assert!(
            files
                .changed(now, |source, _| source != "removed")
                .is_empty()
        );
        assert!(files.files.is_empty());
    }

    #[test]
    fn bounds_candidates_and_keeps_newest_regardless_of_scan_order() {
        use std::fs::{File, FileTimes};
        let dir = tempfile::tempdir().unwrap();
        let mut files = LiveFiles::default();
        for i in (0..LIMIT + 10).rev() {
            let path = dir.path().join(format!("{i}.jsonl"));
            let file = File::create(&path).unwrap();
            file.set_times(
                FileTimes::new()
                    .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(i as u64)),
            )
            .unwrap();
            files.track("macos", &path);
        }
        assert_eq!(files.files.len(), LIMIT);
        assert_eq!(
            files.files.values().map(|s| s.modified).min(),
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(10))
        );
    }

    #[test]
    fn detects_replacement_even_with_identical_length_and_mtime() {
        use std::fs::{File, FileTimes};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        fs::write(&path, "old").unwrap();
        let mut files = LiveFiles::default();
        files.track("macos", &path);
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let replacement = dir.path().join("replacement");
        fs::write(&replacement, "new").unwrap();
        File::open(&replacement)
            .unwrap()
            .set_times(FileTimes::new().set_modified(modified))
            .unwrap();
        fs::rename(replacement, &path).unwrap();
        assert_eq!(
            files.changed(Instant::now() + INTERVAL, |_, _| true),
            vec![("macos".into(), path)]
        );
    }
}
