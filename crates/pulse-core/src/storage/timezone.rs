use super::{Store, StoreError};
use chrono::DateTime;
use chrono_tz::Tz;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimezoneProgress {
    pub target_timezone: String,
    pub processed: u64,
    pub ready: bool,
}
pub(super) struct TimezoneRebuild {
    timezone: Tz,
    after: i64,
    processed: u64,
    ready: bool,
}
impl Store {
    /// Staging belongs to this connection and disappears on cancel or restart.
    /// Only normalized fact columns are read; Codex log files are not rescanned.
    pub fn begin_timezone_rebuild(&mut self, timezone: Tz) -> Result<(), StoreError> {
        let tx = self.connection.transaction()?;
        tx.execute_batch(
            "DROP TABLE IF EXISTS temp.timezone_daily;
            CREATE TEMP TABLE timezone_daily (
                day TEXT NOT NULL, model TEXT NOT NULL,
                total INTEGER NOT NULL CHECK(total>=0),
                input INTEGER NOT NULL CHECK(input>=0),
                cached INTEGER NOT NULL CHECK(cached>=0),
                output INTEGER NOT NULL CHECK(output>=0),
                cost_nanousd INTEGER NOT NULL CHECK(cost_nanousd>=0),
                unpriced_tokens INTEGER NOT NULL CHECK(unpriced_tokens>=0),
                incomplete_events INTEGER NOT NULL CHECK(incomplete_events>=0),
                PRIMARY KEY(day,model)
            ) STRICT;",
        )?;
        tx.commit()?;
        self.timezone_rebuild = Some(TimezoneRebuild {
            timezone,
            after: 0,
            processed: 0,
            ready: false,
        });
        Ok(())
    }
    pub fn timezone_rebuild_status(&self) -> Option<TimezoneProgress> {
        self.timezone_rebuild.as_ref().map(|job| TimezoneProgress {
            target_timezone: job.timezone.name().into(),
            processed: job.processed,
            ready: job.ready,
        })
    }
    pub fn cancel_timezone_rebuild(&mut self) -> Result<(), StoreError> {
        self.connection
            .execute_batch("DROP TABLE IF EXISTS temp.timezone_daily")?;
        self.timezone_rebuild = None;
        Ok(())
    }
    /// At most 512 facts per transaction; callers yield between batches. New
    /// facts appended during rebuilding are consumed by the same rowid cursor.
    pub fn step_timezone_rebuild(&mut self, limit: usize) -> Result<TimezoneProgress, StoreError> {
        if !(1..=512).contains(&limit) {
            return Err(StoreError::Query);
        }
        let job = self
            .timezone_rebuild
            .as_ref()
            .ok_or(StoreError::RebuildPending)?;
        let timezone = job.timezone;
        let after = job.after;
        let rows = {
            let mut query = self.connection.prepare(
                "SELECT rowid,occurred_at,model,total,input,cached,output,cost_nanousd
                FROM usage_facts WHERE rowid>?1 ORDER BY rowid LIMIT ?2",
            )?;
            query
                .query_map(params![after, limit as i64], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        r.get::<_, Option<i64>>(6)?,
                        r.get::<_, Option<i64>>(7)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let tx = self.connection.transaction()?;
        {
            let mut insert = tx.prepare_cached("INSERT INTO temp.timezone_daily
                (day,model,total,input,cached,output,cost_nanousd,unpriced_tokens,incomplete_events)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                ON CONFLICT(day,model) DO UPDATE SET total=total+excluded.total,input=input+excluded.input,
                cached=cached+excluded.cached,output=output+excluded.output,cost_nanousd=cost_nanousd+excluded.cost_nanousd,
                unpriced_tokens=unpriced_tokens+excluded.unpriced_tokens,incomplete_events=incomplete_events+excluded.incomplete_events")?;
            for (_, at, model, total, input, cached, output, cost) in &rows {
                let day = DateTime::parse_from_rfc3339(at)
                    .map_err(|_| StoreError::Time)?
                    .with_timezone(&timezone)
                    .date_naive()
                    .to_string();
                let total = total.unwrap_or(0);
                insert.execute(params![
                    day,
                    model,
                    total,
                    input.unwrap_or(0),
                    cached.unwrap_or(0),
                    output.unwrap_or(0),
                    cost.unwrap_or(0),
                    if cost.is_none() { total } else { 0 },
                    i64::from(input.is_none() || output.is_none())
                ])?;
            }
        }
        tx.commit()?;
        let job = self.timezone_rebuild.as_mut().unwrap();
        job.processed = job
            .processed
            .checked_add(rows.len() as u64)
            .ok_or(crate::domain::DataError::Overflow)?;
        if let Some(last) = rows.last() {
            job.after = last.0;
        }
        job.ready = rows.is_empty();
        Ok(self.timezone_rebuild_status().unwrap())
    }
    /// Publish buckets and settings in one transaction. Old buckets, settings,
    /// fact rows, cursors and session aggregates remain valid on any failure.
    pub fn commit_timezone_rebuild(
        &mut self,
        settings: &[(&str, serde_json::Value)],
    ) -> Result<(), StoreError> {
        let job = self
            .timezone_rebuild
            .as_ref()
            .ok_or(StoreError::RebuildPending)?;
        if !job.ready {
            return Err(StoreError::RebuildPending);
        }
        let tx = self.connection.transaction()?;
        let appended: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM usage_facts WHERE rowid>?1)",
            [job.after],
            |r| r.get(0),
        )?;
        if appended {
            return Err(StoreError::RebuildPending);
        }
        tx.execute("DELETE FROM daily_model_usage", [])?;
        tx.execute("INSERT INTO daily_model_usage(day,timezone,model,total,input,cached,output,cost_nanousd,unpriced_tokens,incomplete_events)
            SELECT day,?1,model,total,input,cached,output,cost_nanousd,unpriced_tokens,incomplete_events FROM temp.timezone_daily", [job.timezone.name()])?;
        for (key, value) in settings {
            tx.execute("INSERT INTO settings(key,json) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET json=excluded.json",params![key,serde_json::to_string(value)?])?;
        }
        tx.execute_batch("DROP TABLE temp.timezone_daily")?;
        tx.commit()?;
        self.timezone_rebuild = None;
        Ok(())
    }
}
