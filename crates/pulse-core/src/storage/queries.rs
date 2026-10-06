use super::{ModelUsage, Store, StoreError, unsigned};
use crate::domain::SessionMeta;
use chrono::{Days, NaiveDate};
use chrono_tz::Tz;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DayUsage {
    pub day: String,
    pub total: u64,
    pub cost_nanousd: i64,
    pub unpriced_tokens: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentSession {
    pub meta: SessionMeta,
    pub models: Vec<String>,
    pub sources: Vec<String>,
    pub total: u64,
    pub events: u64,
    pub unknown_totals: u64,
    pub cost_nanousd: i64,
    pub unpriced_tokens: u64,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    pub from_day: String,
    pub through_day: String,
    pub timezone: String,
    pub total: u64,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub cost_nanousd: i64,
    pub unpriced_tokens: u64,
    pub incomplete_events: u64,
    pub sessions: u64,
    pub models: Vec<ModelUsage>,
    pub days: Vec<DayUsage>,
}
impl Store {
    pub fn setting<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, StoreError> {
        let value: Option<String> = self
            .connection
            .query_row("SELECT json FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        value
            .map(|v| serde_json::from_str(&v).map_err(StoreError::from))
            .transpose()
    }
    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StoreError> {
        self.connection.execute("INSERT INTO settings(key,json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET json=excluded.json",params![key,serde_json::to_string(value)?])?;
        Ok(())
    }
    pub fn set_settings(&mut self, values: &[(&str, serde_json::Value)]) -> Result<(), StoreError> {
        let tx = self.connection.transaction()?;
        for (key, value) in values {
            tx.execute("INSERT INTO settings(key,json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET json=excluded.json",params![key,serde_json::to_string(value)?])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn summary(&self, today: NaiveDate, timezone: Tz) -> Result<UsageSummary, StoreError> {
        let from = today
            .checked_sub_days(Days::new(29))
            .ok_or(StoreError::Time)?;
        let mut result = UsageSummary {
            from_day: from.to_string(),
            through_day: today.to_string(),
            timezone: timezone.name().into(),
            ..Default::default()
        };
        result.models = self.model_usage(&result.from_day, &result.through_day, timezone)?;
        // SQLite aggregates are checked integers. Rust sums are checked too; overflow is a
        // surfaced data failure, never a wrapped or silently rounded display total.
        for model in &result.models {
            macro_rules! add {
                ($field:ident) => {
                    result.$field = result
                        .$field
                        .checked_add(model.$field)
                        .ok_or(crate::domain::DataError::Overflow)?
                };
            }
            add!(total);
            add!(input);
            add!(cached);
            add!(output);
            add!(cost_nanousd);
            add!(unpriced_tokens);
            add!(incomplete_events);
        }
        result.sessions =
            self.independent_sessions(&result.from_day, &result.through_day, timezone)?;
        let mut query=self.connection.prepare("SELECT SUM(total),SUM(cost_nanousd),SUM(unpriced_tokens) FROM daily_model_usage WHERE day=?1 AND timezone=?2 HAVING COUNT(*)>0")?;
        for offset in 0..30 {
            let day = from
                .checked_add_days(Days::new(offset))
                .ok_or(StoreError::Time)?
                .to_string();
            let bucket = query
                .query_row(params![day, timezone.name()], |row| {
                    Ok(DayUsage {
                        day: day.clone(),
                        total: unsigned(row, 0)?,
                        cost_nanousd: row.get(1)?,
                        unpriced_tokens: unsigned(row, 2)?,
                    })
                })
                .optional()?;
            result.days.push(bucket.unwrap_or(DayUsage {
                day,
                ..Default::default()
            }));
        }
        Ok(result)
    }
    pub fn recent_sessions(
        &self,
        before: Option<(&str, &str)>,
        limit: u32,
    ) -> Result<Vec<RecentSession>, StoreError> {
        let (time, id) = before.unwrap_or(("9999", "~"));
        let mut query=self.connection.prepare("SELECT id FROM sessions WHERE (last_activity,id)<(?1,?2) ORDER BY last_activity DESC,id DESC LIMIT ?3")?;
        let ids = query
            .query_map(params![time, id, limit.clamp(1, 100)], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        ids.iter()
            .map(|id| self.session_by_id(id)?.ok_or(StoreError::Query))
            .collect()
    }
}
