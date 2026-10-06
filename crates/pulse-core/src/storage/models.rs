use super::{ModelUsage, Store, StoreError, day_boundary, day_boundary_next, unsigned};
use chrono::NaiveDate;
use chrono_tz::Tz;
use rusqlite::params;
use serde::{Deserialize, Serialize};

const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCursor {
    // Decimal strings preserve exact SQLite integers across JavaScript IPC.
    pub total: String,
    pub model: String,
    pub watermark: String,
    pub from_day: String,
    pub through_day: String,
    pub timezone: String,
}
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDirection {
    #[default]
    Next,
    Previous,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelPageRequest {
    pub from_day: String,
    pub through_day: String,
    pub cursor: Option<ModelCursor>,
    #[serde(default)]
    pub direction: ModelDirection,
}
impl ModelPageRequest {
    pub fn validate(&self) -> Result<(), StoreError> {
        let date = |s: &str| {
            if s.len() != 10 {
                return Err(StoreError::Query);
            }
            NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| StoreError::Query)
        };
        let span = date(&self.through_day)?
            .signed_duration_since(date(&self.from_day)?)
            .num_days();
        if !(0..=365).contains(&span)
            || matches!(self.direction, ModelDirection::Previous) && self.cursor.is_none()
        {
            return Err(StoreError::Query);
        }
        if let Some(cursor) = &self.cursor {
            if cursor.model.is_empty()
                || cursor.model.len() > 4096
                || cursor.from_day != self.from_day
                || cursor.through_day != self.through_day
                || cursor.timezone.len() > 64
                || cursor.timezone.parse::<Tz>().is_err()
            {
                return Err(StoreError::Query);
            }
            decimal(&cursor.total)?;
            decimal(&cursor.watermark)?;
        }
        Ok(())
    }
}
fn decimal(value: &str) -> Result<i64, StoreError> {
    if value.is_empty() || value.len() > 19 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(StoreError::Query);
    }
    value.parse().map_err(|_| StoreError::Query)
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelRow {
    pub reference: super::ReferenceEstimate,
    #[serde(flatten)]
    pub usage: ModelUsage,
    pub events: u64,
    pub unknown_totals: u64,
    pub unknown_input: u64,
    pub unknown_cached: u64,
    pub unknown_output: u64,
    pub unpriced_events: u64,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelPage {
    pub items: Vec<ModelRow>,
    pub next: Option<ModelCursor>,
    pub previous: Option<ModelCursor>,
    pub watermark: String,
    pub total_models: u64,
    pub known_total: u64,
    pub unknown_total_events: u64,
}
impl Store {
    pub fn fact_revision(&self) -> Result<String, StoreError> {
        Ok(self
            .connection
            .query_row(
                "SELECT COALESCE(MAX(rowid),0) FROM usage_facts",
                [],
                |row| row.get::<_, i64>(0),
            )?
            .to_string())
    }
    pub fn model_page(
        &self,
        request: &ModelPageRequest,
        timezone: Tz,
    ) -> Result<ModelPage, StoreError> {
        request.validate()?;
        let current = decimal(&self.fact_revision()?)?;
        let (watermark, total, model) = match &request.cursor {
            Some(cursor) if cursor.timezone == timezone.name() => (
                decimal(&cursor.watermark)?,
                decimal(&cursor.total)?,
                cursor.model.as_str(),
            ),
            Some(_) => return Err(StoreError::Query),
            None => (current, 0, ""),
        };
        if watermark > current {
            return Err(StoreError::Query);
        }
        let (op, order) = match request.direction {
            ModelDirection::Next => (
                "total<?4 OR (total=?4 AND model>?5)",
                "total DESC,model ASC",
            ),
            ModelDirection::Previous => (
                "total>?4 OR (total=?4 AND model<?5)",
                "total ASC,model DESC",
            ),
        };
        // The immutable, append-only ledger watermark fixes membership and sort keys
        // across pages while ingestion proceeds. One grouped query counts sessions;
        // session counts use this single query; reference reads are bounded by page size.
        // No JSON decoding or unbounded IPC arrays are involved.
        let sql = format!("WITH subagent_sessions AS MATERIALIZED (SELECT id FROM sessions WHERE json_extract(metadata,'$.source_kind')='subagent'), grouped AS (
            SELECT model,COALESCE(SUM(total),0) AS total,COALESCE(SUM(input),0) AS input,
                COALESCE(SUM(cached),0) AS cached,COALESCE(SUM(output),0) AS output,
                COALESCE(SUM(cost_nanousd),0) AS cost,COALESCE(SUM(CASE WHEN cost_nanousd IS NULL THEN total ELSE 0 END),0) AS unpriced,
                SUM(CASE WHEN input IS NULL OR output IS NULL THEN 1 ELSE 0 END) AS incomplete,COUNT(DISTINCT CASE WHEN session_id NOT IN (SELECT id FROM subagent_sessions) THEN session_id END) AS sessions,
                COUNT(*) AS events,COUNT(*)-COUNT(total) AS unknown_totals,COUNT(*)-COUNT(input) AS unknown_input,
                COUNT(*)-COUNT(cached) AS unknown_cached,COUNT(*)-COUNT(output) AS unknown_output,COUNT(*)-COUNT(cost_nanousd) AS unpriced_events
            FROM usage_facts WHERE occurred_at>=?1 AND occurred_at<?2 AND rowid<=?3 GROUP BY model
        ), ranked AS (
            SELECT *,ROW_NUMBER() OVER (ORDER BY total DESC,model ASC) AS position,COUNT(*) OVER () AS model_count,
                SUM(total) OVER () AS known_total,SUM(unknown_totals) OVER () AS unknown_total_events FROM grouped
        ) SELECT model,total,input,cached,output,cost,unpriced,incomplete,sessions,position,model_count,events,unknown_totals,unknown_input,unknown_cached,unknown_output,unpriced_events,known_total,unknown_total_events
            FROM ranked WHERE (?6 OR ({op})) ORDER BY {order} LIMIT ?7");
        let mut query = self.connection.prepare(&sql)?;
        let mut rows = query
            .query_map(
                params![
                    day_boundary(&request.from_day, timezone)?,
                    day_boundary_next(&request.through_day, timezone)?,
                    watermark,
                    total,
                    model,
                    request.cursor.is_none(),
                    (PAGE_SIZE + 1) as i64
                ],
                |row| {
                    Ok((
                        ModelRow {
                            usage: ModelUsage {
                                model: row.get(0)?,
                                total: unsigned(row, 1)?,
                                input: unsigned(row, 2)?,
                                cached: unsigned(row, 3)?,
                                output: unsigned(row, 4)?,
                                cost_nanousd: row.get(5)?,
                                unpriced_tokens: unsigned(row, 6)?,
                                incomplete_events: unsigned(row, 7)?,
                                sessions: unsigned(row, 8)?,
                            },
                            events: unsigned(row, 11)?,
                            unknown_totals: unsigned(row, 12)?,
                            unknown_input: unsigned(row, 13)?,
                            unknown_cached: unsigned(row, 14)?,
                            unknown_output: unsigned(row, 15)?,
                            unpriced_events: unsigned(row, 16)?,
                            reference: Default::default(),
                        },
                        unsigned(row, 9)?,
                        unsigned(row, 10)?,
                        unsigned(row, 17)?,
                        unsigned(row, 18)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.truncate(PAGE_SIZE);
        for row in &mut rows {
            row.0.reference = self.model_reference(
                &day_boundary(&request.from_day, timezone)?,
                &day_boundary_next(&request.through_day, timezone)?,
                &row.0.usage.model,
                watermark,
            )?;
        }
        if matches!(request.direction, ModelDirection::Previous) {
            rows.reverse();
        }
        let cursor = |row: &(ModelRow, u64, u64, u64, u64)| ModelCursor {
            total: row.0.usage.total.to_string(),
            model: row.0.usage.model.clone(),
            watermark: watermark.to_string(),
            from_day: request.from_day.clone(),
            through_day: request.through_day.clone(),
            timezone: timezone.name().into(),
        };
        Ok(ModelPage {
            next: rows.last().filter(|row| row.1 < row.2).map(cursor),
            previous: rows.first().filter(|row| row.1 > 1).map(cursor),
            total_models: rows.first().map_or(0, |row| row.2),
            known_total: rows.first().map_or(0, |row| row.3),
            unknown_total_events: rows.first().map_or(0, |row| row.4),
            items: rows.into_iter().map(|row| row.0).collect(),
            watermark: watermark.to_string(),
        })
    }
}
