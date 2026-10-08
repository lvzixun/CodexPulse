use super::{ModelPageRequest, Store, StoreError, day_boundary, day_boundary_next, unsigned};
use crate::domain::RunSample;
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedRange {
    #[default]
    Recent50,
    Recent100,
    Month,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpeedRequest {
    pub model: String,
    pub from_day: String,
    pub through_day: String,
    #[serde(default)]
    pub range: SpeedRange,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SpeedSummary {
    pub output_tokens: u64,
    pub elapsed_ms: u64,
    pub samples: u64,
    pub service_tier: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedPoint {
    pub started_at: String,
    pub measured_at: String,
    #[serde(flatten)]
    pub summary: SpeedSummary,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelSpeed {
    #[serde(flatten)]
    pub summary: SpeedSummary,
    pub points: Vec<SpeedPoint>,
    pub current: Option<SpeedPoint>,
    pub history_pending: bool,
}

pub(super) fn commit_runs(tx: &Transaction<'_>, samples: &[RunSample]) -> Result<(), StoreError> {
    for sample in samples {
        let rate = &sample.rate;
        let start =
            DateTime::parse_from_rfc3339(&sample.started_at).map_err(|_| StoreError::Time)?;
        let end = DateTime::parse_from_rfc3339(&rate.measured_at).map_err(|_| StoreError::Time)?;
        let observed =
            u64::try_from((end - start).num_milliseconds()).map_err(|_| StoreError::Time)?;
        if rate.output_tokens == 0
            || rate.elapsed_ms == 0
            || observed.abs_diff(rate.elapsed_ms) > 2000
            || sample.model.is_empty()
            || sample.id.is_empty()
            || sample.session_id.is_empty()
        {
            return Err(StoreError::Query);
        }
        let output = i64::try_from(rate.output_tokens).map_err(|_| StoreError::Query)?;
        let elapsed = i64::try_from(rate.elapsed_ms).map_err(|_| StoreError::Query)?;
        tx.execute("INSERT INTO run_speeds VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
            ON CONFLICT(id) DO UPDATE SET measured_at=excluded.measured_at,output_tokens=excluded.output_tokens,
                elapsed_ms=excluded.elapsed_ms,service_tier=excluded.service_tier,completed=excluded.completed
            WHERE excluded.completed>=run_speeds.completed AND excluded.measured_at>=run_speeds.measured_at",
            params![sample.id,sample.session_id,sample.model,super::canonical_activity(&sample.started_at),
                super::canonical_activity(&rate.measured_at),output,elapsed,rate.service_tier,rate.completed])?;
    }
    Ok(())
}
fn point(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpeedPoint> {
    Ok(SpeedPoint {
        started_at: row.get(0)?,
        measured_at: row.get(1)?,
        summary: SpeedSummary {
            output_tokens: unsigned(row, 2)?,
            elapsed_ms: unsigned(row, 3)?,
            samples: unsigned(row, 4)?,
            service_tier: row.get(5)?,
        },
    })
}
impl Store {
    pub fn model_speed(
        &self,
        request: &ModelSpeedRequest,
        timezone: Tz,
        now: DateTime<Utc>,
        connected: &[String],
    ) -> Result<ModelSpeed, StoreError> {
        if request.model.is_empty() || request.model.len() > 4096 {
            return Err(StoreError::Query);
        }
        ModelPageRequest {
            from_day: request.from_day.clone(),
            through_day: request.through_day.clone(),
            cursor: None,
            direction: Default::default(),
        }
        .validate()?;
        let from = day_boundary(&request.from_day, timezone)?;
        let through = day_boundary_next(&request.through_day, timezone)?;
        // A single latest, fresh owned run is shown as current, never the sum of parallel tasks.
        let mut current_query=self.connection.prepare("SELECT r.started_at,r.measured_at,r.output_tokens,r.elapsed_ms,1,r.service_tier
            FROM run_speeds r JOIN sessions s ON s.id=r.session_id
            WHERE r.model=?1 AND r.completed=0 AND r.measured_at>=?2 AND r.measured_at<?3
              AND r.measured_at>=?4 AND r.measured_at<=?5
              AND json_extract(s.metadata,'$.status')='active'
              AND json_extract(s.metadata,'$.current_model')=r.model
              AND json_extract(s.metadata,'$.output_rate.completed')=0
              AND json_extract(s.metadata,'$.output_rate.output_tokens')=r.output_tokens
              AND json_extract(s.metadata,'$.output_rate.elapsed_ms')=r.elapsed_ms
              AND ABS(julianday(json_extract(s.metadata,'$.output_rate.measured_at'))-julianday(r.measured_at))*86400<0.001
              AND EXISTS(SELECT 1 FROM source_sessions ss WHERE ss.session_id=s.id
                  AND ss.source_id IN (SELECT value FROM json_each(?6)))
            ORDER BY r.measured_at DESC,r.id DESC LIMIT 1")?;
        let current = current_query
            .query_map(
                params![
                    request.model,
                    from,
                    through,
                    super::canonical_activity(&(now - Duration::seconds(60)).to_rfc3339()),
                    super::canonical_activity(&now.to_rfc3339()),
                    serde_json::to_string(connected)?
                ],
                point,
            )?
            .collect::<Result<Vec<_>, _>>()?
            .pop();
        let mut points = match request.range {
            SpeedRange::Recent50 | SpeedRange::Recent100 => {
                let limit = if matches!(request.range, SpeedRange::Recent50) {
                    50
                } else {
                    100
                };
                let mut query=self.connection.prepare("SELECT started_at,measured_at,output_tokens,elapsed_ms,1,service_tier FROM run_speeds WHERE model=?1 AND completed=1 AND measured_at>=?2 AND measured_at<?3 AND measured_at<=?4 ORDER BY measured_at DESC,id DESC LIMIT ?5")?;
                let mut result = query
                    .query_map(
                        params![
                            request.model,
                            from,
                            through,
                            super::canonical_activity(&now.to_rfc3339()),
                            limit
                        ],
                        point,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                result.reverse();
                if let Some(current) = &current {
                    result.push(current.clone());
                }
                result.sort_by(|a, b| a.measured_at.cmp(&b.measured_at));
                if result.len() > limit as usize {
                    result.drain(..result.len() - limit as usize);
                }
                result
            }
            SpeedRange::Month => {
                // Fixed UTC buckets are formatted in the requested timezone by the UI.
                // At most ~120 points, even if an external client requests a longer range.
                let span = (DateTime::parse_from_rfc3339(&through)
                    .map_err(|_| StoreError::Time)?
                    - DateTime::parse_from_rfc3339(&from).map_err(|_| StoreError::Time)?)
                .num_seconds();
                let bucket = 21600_i64.max((span + 119) / 120);
                let mut query=self.connection.prepare("SELECT MIN(started_at),MAX(measured_at),SUM(output_tokens),SUM(elapsed_ms),COUNT(*),CASE WHEN COUNT(DISTINCT COALESCE(service_tier,'unknown'))=1 THEN MAX(service_tier) ELSE 'mixed' END FROM run_speeds WHERE model=?1 AND completed=1 AND measured_at>=?2 AND measured_at<?3 AND measured_at<=?4 GROUP BY CAST(strftime('%s',measured_at) AS INTEGER)/?5 ORDER BY MAX(measured_at)")?;
                let mut result = query
                    .query_map(
                        params![
                            request.model,
                            from,
                            through,
                            super::canonical_activity(&now.to_rfc3339()),
                            bucket
                        ],
                        point,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(current) = &current {
                    result.push(current.clone());
                }
                result.sort_by(|a, b| a.measured_at.cmp(&b.measured_at));
                result
            }
        };
        // All ratios are weighted by observed run duration, never by wall-clock idle time.
        let mut summary = SpeedSummary::default();
        let mut tiers = std::collections::BTreeSet::new();
        for p in &points {
            summary.output_tokens = summary
                .output_tokens
                .checked_add(p.summary.output_tokens)
                .ok_or(StoreError::Query)?;
            summary.elapsed_ms = summary
                .elapsed_ms
                .checked_add(p.summary.elapsed_ms)
                .ok_or(StoreError::Query)?;
            summary.samples = summary
                .samples
                .checked_add(p.summary.samples)
                .ok_or(StoreError::Query)?;
            tiers.insert(p.summary.service_tier.as_deref().unwrap_or("unknown"));
        }
        summary.service_tier = if tiers.len() > 1 {
            Some("mixed".into())
        } else {
            tiers
                .first()
                .filter(|v| **v != "unknown")
                .map(|v| (*v).to_owned())
        };
        // Drop no-sample points rather than inventing zero speed.
        points.retain(|p| p.summary.output_tokens > 0 && p.summary.elapsed_ms > 0);
        Ok(ModelSpeed {
            summary,
            points,
            current,
            history_pending: false,
        })
    }
}
