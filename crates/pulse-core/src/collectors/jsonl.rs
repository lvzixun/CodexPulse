//! Bounded, restartable JSONL tailing. Only committed complete records advance the cursor.
use crate::{
    PARSER_VERSION,
    ledger::ParserState,
    pricing::PriceBook,
    storage::{FileCursor, Store, StoreError},
};
use chrono_tz::Tz;
use std::{
    collections::BTreeMap,
    fs::{File, Metadata},
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
    time::{Duration, Instant, UNIX_EPOCH},
};

pub const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_BATCH_BYTES: usize = 8 * 1024 * 1024;
const MAX_BATCH_RECORDS: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
}
#[derive(Debug, Default)]
pub struct ReadReport {
    pub bytes_read: usize,
    pub records: usize,
    pub inserted: usize,
    pub issues: usize,
    pub more: bool,
    pub unchanged: bool,
}

fn identity(meta: &Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return format!("{}:{}", meta.dev(), meta.ino());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        format!("{}", meta.creation_time())
    }
    #[cfg(not(any(unix, windows)))]
    {
        format!("{:?}", meta.created().ok())
    }
}
fn modified(meta: &Metadata) -> String {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default()
}

pub fn read_batch(
    store: &mut Store,
    source: &str,
    path: &Path,
    timezone: Tz,
    prices: &PriceBook,
) -> Result<ReadReport, ReadError> {
    read_inner(store, source, path, timezone, prices, false)
}
pub fn read_titles(store: &mut Store, source: &str, path: &Path) -> Result<ReadReport, ReadError> {
    read_inner(
        store,
        source,
        path,
        chrono_tz::UTC,
        &PriceBook::default(),
        true,
    )
}
fn read_inner(
    store: &mut Store,
    source: &str,
    path: &Path,
    timezone: Tz,
    prices: &PriceBook,
    titles_only: bool,
) -> Result<ReadReport, ReadError> {
    let meta = path.metadata()?;
    let key = if titles_only {
        format!("session-index:{}", path.to_string_lossy())
    } else {
        path.to_string_lossy().into_owned()
    };
    let file_identity = identity(&meta);
    let modified_ns = modified(&meta);
    let prior = store.cursor(source, &key)?;
    let mut cursor = prior
        .filter(|c| {
            c.parser_version == PARSER_VERSION
                && c.file_identity == file_identity
                && c.offset <= meta.len()
        })
        .unwrap_or_else(|| FileCursor {
            source_id: source.into(),
            file_key: key,
            offset: 0,
            file_identity,
            modified_ns: String::new(),
            parser_version: PARSER_VERSION,
            state: ParserState::default(),
        });
    if !modified_ns.is_empty()
        && cursor.modified_ns == modified_ns
        && cursor.state.observed_file_len == Some(meta.len())
    {
        return Ok(ReadReport {
            unchanged: true,
            ..Default::default()
        });
    }
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(cursor.offset))?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut report = ReadReport::default();
    let mut line = Vec::with_capacity(4096);
    let mut position = cursor.offset;
    let mut line_start = position;
    let mut sessions = BTreeMap::new();
    let mut facts = Vec::new();
    let mut titles = Vec::new();
    let mut issues = Vec::new();
    let started = Instant::now();
    let mut eof = false;
    loop {
        if report.bytes_read >= MAX_BATCH_BYTES
            || report.records >= MAX_BATCH_RECORDS
            || started.elapsed() >= Duration::from_millis(250)
        {
            break;
        }
        let available = reader.fill_buf()?;
        if available.is_empty() {
            eof = true;
            break;
        }
        let remaining = MAX_BATCH_BYTES - report.bytes_read;
        let segment = &available[..available.len().min(remaining)];
        let newline = segment.iter().position(|b| *b == b'\n');
        let count = newline.map_or(segment.len(), |n| n + 1);
        if !cursor.state.discarding_line {
            if line.len() + count > MAX_RECORD_BYTES {
                cursor.state.discarding_line = true;
                issues.push("oversized_record".into());
                line.clear();
            } else {
                line.extend_from_slice(&segment[..count]);
            }
        }
        reader.consume(count);
        position += count as u64;
        report.bytes_read += count;
        if newline.is_some() {
            report.records += 1;
            if cursor.state.discarding_line {
                cursor.state.discarding_line = false;
            } else if !line.iter().all(u8::is_ascii_whitespace) {
                if titles_only {
                    match crate::storage::SessionTitle::parse(&line) {
                        Ok(title) => titles.push(title),
                        Err(code) => issues.push(code.into()),
                    }
                } else {
                    let parsed = cursor.state.parse(&line);
                    if let Some(session) = parsed.session {
                        sessions.insert(session.id.clone(), session);
                    }
                    if let Some(mut fact) = parsed.fact {
                        prices.apply(&mut fact);
                        facts.push(fact);
                    }
                    if let Some(issue) = parsed.issue {
                        issues.push(issue.code);
                    }
                }
            }
            line.clear();
            line_start = position;
        }
    }
    // An incomplete JSON record is retried after the next append. Oversized bodies can be
    // checkpointed mid-line because the discard flag is stored in the same transaction.
    cursor.offset = if line.is_empty() || cursor.state.discarding_line {
        position
    } else {
        line_start
    };
    report.more = !eof && position < meta.len();
    cursor.modified_ns = if report.more {
        String::new()
    } else {
        modified_ns
    };
    cursor.state.observed_file_len = Some(meta.len());
    report.issues = issues.len();
    report.inserted = if titles_only {
        store.commit_titles(&cursor, &titles, &issues)?
    } else {
        store.commit_batch(
            &cursor,
            &sessions.into_values().collect::<Vec<_>>(),
            &facts,
            &issues,
            timezone,
        )?
    };
    Ok(report)
}
