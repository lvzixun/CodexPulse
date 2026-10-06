use crate::{
    PARSER_VERSION,
    domain::{SessionMeta, UsageFact},
    ledger::ParserState,
};
use chrono::DateTime;
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
mod queries;
mod sessions;
mod titles;
pub use queries::{DayUsage, RecentSession, UsageSummary};
pub use sessions::{
    PageDirection, PriceCoverage, SessionCursor, SessionDetail, SessionDetailRequest,
    SessionFilter, SessionModel, SessionPage, SessionPageRequest, TokenMeasure, UsageBreakdown,
};
pub use titles::SessionTitle;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Invalid(#[from] crate::domain::DataError),
    #[error("invalid usage timestamp")]
    Time,
    #[error("database schema is newer than this application supports")]
    NewerSchema,
    #[error("invalid query parameters")]
    Query,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCursor {
    pub source_id: String,
    pub file_key: String,
    pub offset: u64,
    pub file_identity: String,
    pub modified_ns: String,
    pub parser_version: u32,
    pub state: ParserState,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub total: u64,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub cost_nanousd: i64,
    pub unpriced_tokens: u64,
    pub incomplete_events: u64,
    pub sessions: u64,
}

pub struct Store {
    connection: Connection,
}
impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY)",
        )?;
        let version: u32 = connection.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_version",
            [],
            |row| row.get(0),
        )?;
        if version > 4 {
            return Err(StoreError::NewerSchema);
        }
        if version < 3 {
            let tx = connection.transaction()?;
            if version < 1 {
                tx.execute_batch(include_str!("../../migrations/001.sql"))?;
            }
            if version < 2 {
                tx.execute_batch(include_str!("../../migrations/002-query-indexes.sql"))?;
            }
            tx.execute_batch(include_str!("../../migrations/003-session-integrity.sql"))?;
            // Normalize legacy sort keys in bounded batches, including metadata used by cursors.
            let mut after: Option<String> = None;
            loop {
                let rows = {
                    let mut query = tx.prepare("SELECT id,metadata,last_activity FROM sessions WHERE (?1 IS NULL OR id>?1) ORDER BY id LIMIT 128")?;
                    query
                        .query_map([&after], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                            ))
                        })?
                        .collect::<Result<Vec<_>, _>>()?
                };
                if rows.is_empty() {
                    break;
                }
                for (id, json, activity) in &rows {
                    let mut meta: SessionMeta = serde_json::from_str(json)?;
                    let canonical = canonical_activity(activity);
                    if *activity != canonical || meta.last_activity != canonical {
                        meta.last_activity = canonical.clone();
                        tx.execute(
                            "UPDATE sessions SET metadata=?2,last_activity=?3 WHERE id=?1",
                            params![id, serde_json::to_string(&meta)?, canonical],
                        )?;
                    }
                }
                after = rows.last().map(|r| r.0.clone());
            }
            tx.commit()?;
        }
        if version < 4 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../../migrations/004-session-titles.sql"))?;
            tx.commit()?;
        }
        Ok(Self { connection })
    }
    pub fn in_memory() -> Result<Self, StoreError> {
        Self::open(":memory:")
    }
    pub fn cursor(
        &self,
        source_id: &str,
        file_key: &str,
    ) -> Result<Option<FileCursor>, StoreError> {
        let result = self.connection.query_row(
            "SELECT offset,file_identity,modified_ns,parser_version,parser_state FROM file_cursors WHERE source_id=?1 AND file_key=?2",
            params![source_id, file_key], |row| Ok((unsigned(row,0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,u32>(3)?,row.get::<_,String>(4)?))
        ).optional()?;
        result
            .map(|(offset, identity, modified, version, json)| {
                Ok(FileCursor {
                    source_id: source_id.into(),
                    file_key: file_key.into(),
                    offset,
                    file_identity: identity,
                    modified_ns: modified,
                    parser_version: version,
                    state: serde_json::from_str(&json)?,
                })
            })
            .transpose()
    }
    /// Facts, derived buckets, metadata and checkpoint advance atomically.
    pub fn commit_batch(
        &mut self,
        cursor: &FileCursor,
        sessions: &[SessionMeta],
        facts: &[UsageFact],
        issues: &[String],
        timezone: Tz,
    ) -> Result<usize, StoreError> {
        let offset =
            i64::try_from(cursor.offset).map_err(|_| crate::domain::DataError::Overflow)?;
        let tx = self.connection.transaction()?;
        for session in sessions {
            let mut session = session.clone();
            session.last_activity = canonical_activity(&session.last_activity);
            tx.execute("INSERT INTO sessions(id,metadata,last_activity) VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET metadata=excluded.metadata,last_activity=excluded.last_activity WHERE excluded.last_activity>=sessions.last_activity",params![session.id,serde_json::to_string(&session)?,session.last_activity])?;
            tx.execute(
                "INSERT OR IGNORE INTO source_sessions VALUES (?1,?2)",
                params![cursor.source_id, session.id],
            )?;
        }
        let mut inserted = 0;
        for fact in facts {
            fact.tokens.validate()?;
            let occurred =
                DateTime::parse_from_rfc3339(&fact.timestamp).map_err(|_| StoreError::Time)?;
            let canonical_time = occurred
                .with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let day = occurred
                .with_timezone(&timezone)
                .format("%Y-%m-%d")
                .to_string();
            let tokens = &fact.tokens;
            let added = tx.execute("INSERT OR IGNORE INTO usage_facts(id,session_id,model,occurred_at,total,input,cached,output,cost_nanousd,json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![fact.id,fact.session_id,fact.model,canonical_time,tokens.effective_total().map(|v|v as i64),tokens.input.map(|v|v as i64),tokens.cached.map(|v|v as i64),tokens.output.map(|v|v as i64),fact.cost_nanousd,serde_json::to_string(fact)?])?;
            if added == 0 {
                continue;
            }
            inserted += 1;
            let total = tokens.effective_total().unwrap_or(0) as i64;
            let unpriced = if fact.cost_nanousd.is_none() {
                total
            } else {
                0
            };
            let incomplete = i64::from(tokens.input.is_none() || tokens.output.is_none());
            tx.execute("INSERT INTO daily_model_usage(day,timezone,model,total,input,cached,output,cost_nanousd,unpriced_tokens,incomplete_events) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(day,timezone,model) DO UPDATE SET total=total+excluded.total,input=input+excluded.input,cached=cached+excluded.cached,output=output+excluded.output,cost_nanousd=cost_nanousd+excluded.cost_nanousd,unpriced_tokens=unpriced_tokens+excluded.unpriced_tokens,incomplete_events=incomplete_events+excluded.incomplete_events",params![day,timezone.name(),fact.model,total,tokens.input.unwrap_or(0) as i64,tokens.cached.unwrap_or(0) as i64,tokens.output.unwrap_or(0) as i64,fact.cost_nanousd.unwrap_or(0),unpriced,incomplete])?;
            tx.execute("INSERT INTO session_model_usage(session_id,model,total,input,cached,output,cost_nanousd,unpriced_tokens) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(session_id,model) DO UPDATE SET total=total+excluded.total,input=input+excluded.input,cached=cached+excluded.cached,output=output+excluded.output,cost_nanousd=cost_nanousd+excluded.cost_nanousd,unpriced_tokens=unpriced_tokens+excluded.unpriced_tokens",params![fact.session_id,fact.model,total,tokens.input.unwrap_or(0) as i64,tokens.cached.unwrap_or(0) as i64,tokens.output.unwrap_or(0) as i64,fact.cost_nanousd.unwrap_or(0),unpriced])?;
        }
        for code in issues {
            tx.execute("INSERT INTO parse_issues(source_id,file_key,code) VALUES(?1,?2,?3) ON CONFLICT(source_id,file_key,code) DO UPDATE SET count=count+1",params![cursor.source_id,cursor.file_key,code])?;
        }
        tx.execute("INSERT INTO file_cursors VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(source_id,file_key) DO UPDATE SET offset=excluded.offset,file_identity=excluded.file_identity,modified_ns=excluded.modified_ns,parser_version=excluded.parser_version,parser_state=excluded.parser_state",params![cursor.source_id,cursor.file_key,offset,cursor.file_identity,cursor.modified_ns,PARSER_VERSION,serde_json::to_string(&cursor.state)?])?;
        tx.commit()?;
        Ok(inserted)
    }
    pub fn model_usage(
        &self,
        from_day: &str,
        through_day: &str,
        timezone: Tz,
    ) -> Result<Vec<ModelUsage>, StoreError> {
        let mut query=self.connection.prepare("SELECT model,SUM(total),SUM(input),SUM(cached),SUM(output),SUM(cost_nanousd),SUM(unpriced_tokens),SUM(incomplete_events) FROM daily_model_usage WHERE day>=?1 AND day<=?2 AND timezone=?3 GROUP BY model ORDER BY SUM(total) DESC,model")?;
        let rows = query.query_map(params![from_day, through_day, timezone.name()], |row| {
            Ok(ModelUsage {
                model: row.get(0)?,
                total: unsigned(row, 1)?,
                input: unsigned(row, 2)?,
                cached: unsigned(row, 3)?,
                output: unsigned(row, 4)?,
                cost_nanousd: row.get(5)?,
                unpriced_tokens: unsigned(row, 6)?,
                incomplete_events: unsigned(row, 7)?,
                sessions: 0,
            })
        })?;
        let mut models = rows.collect::<Result<Vec<_>, _>>()?;
        for model in &mut models {
            // DISTINCT session count is derived from facts within this calendar range.
            model.sessions=self.connection.query_row("SELECT COUNT(DISTINCT session_id) FROM usage_facts WHERE model=?1 AND occurred_at>=?2 AND occurred_at<?3",params![model.model,day_boundary(from_day,timezone)?,day_boundary_next(through_day,timezone)?],|r|unsigned(r,0))?;
        }
        Ok(models)
    }
    pub fn independent_sessions(
        &self,
        from_day: &str,
        through_day: &str,
        timezone: Tz,
    ) -> Result<u64, StoreError> {
        Ok(self.connection.query_row("SELECT COUNT(DISTINCT session_id) FROM usage_facts WHERE occurred_at>=?1 AND occurred_at<?2",params![day_boundary(from_day,timezone)?,day_boundary_next(through_day,timezone)?],|r|unsigned(r,0))?)
    }
    pub fn fact_count(&self) -> Result<u64, StoreError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM usage_facts", [], |r| unsigned(r, 0))?)
    }
}

fn unsigned(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value = row.get::<_, i64>(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn canonical_activity(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| {
            dt.with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
        })
        .unwrap_or_default()
}

fn boundary(day: chrono::NaiveDate, timezone: Tz) -> Result<String, StoreError> {
    use chrono::TimeZone;
    let dt = timezone
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).ok_or(StoreError::Time)?)
        .earliest()
        .ok_or(StoreError::Time)?;
    Ok(dt
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
fn day_boundary(day: &str, timezone: Tz) -> Result<String, StoreError> {
    boundary(
        chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|_| StoreError::Time)?,
        timezone,
    )
}
fn day_boundary_next(day: &str, timezone: Tz) -> Result<String, StoreError> {
    boundary(
        chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .map_err(|_| StoreError::Time)?
            .succ_opt()
            .ok_or(StoreError::Time)?,
        timezone,
    )
}
