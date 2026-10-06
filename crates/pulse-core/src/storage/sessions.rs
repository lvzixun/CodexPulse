use super::{RecentSession, Store, StoreError, day_boundary, day_boundary_next, unsigned};
use chrono_tz::Tz;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCursor {
    pub activity: String,
    pub id: String,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub as_of: Option<String>,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SessionFilter {
    pub model: Option<String>,
    pub from_day: Option<String>,
    pub through_day: Option<String>,
}
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageDirection {
    #[default]
    Older,
    Newer,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SessionPageRequest {
    #[serde(default)]
    pub filter: SessionFilter,
    pub cursor: Option<SessionCursor>,
    #[serde(default)]
    pub direction: PageDirection,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SessionPage {
    pub items: Vec<RecentSession>,
    pub older: Option<SessionCursor>,
    pub newer: Option<SessionCursor>,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenMeasure {
    pub known: u64,
    pub unknown_events: u64,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct UsageBreakdown {
    pub events: u64,
    pub total: TokenMeasure,
    pub input: TokenMeasure,
    pub cached: TokenMeasure,
    pub output: TokenMeasure,
    pub reasoning: TokenMeasure,
    pub cache_write: TokenMeasure,
    pub cost_nanousd: i64,
    pub unpriced_tokens: u64,
    pub unpriced_events: u64,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionModel {
    pub model: String,
    pub usage: UsageBreakdown,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceCoverage {
    pub version: Option<String>,
    pub events: u64,
    pub tokens: u64,
    pub unknown_totals: u64,
    pub reference: Option<crate::pricing::Price>,
    pub cost_nanousd: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetailRequest {
    pub id: String,
    pub models_after: Option<String>,
    pub prices_after: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub session: RecentSession,
    pub usage: UsageBreakdown,
    pub models: Vec<SessionModel>,
    pub models_next: Option<String>,
    pub prices: Vec<PriceCoverage>,
    pub prices_next: Option<String>,
}

// NULL remains unknown for each individual counter, rather than silently becoming zero.
const BREAKDOWN: &str = "COUNT(*), COALESCE(SUM(total),0),COUNT(*)-COUNT(total),COALESCE(SUM(input),0),COUNT(*)-COUNT(input),COALESCE(SUM(cached),0),COUNT(*)-COUNT(cached),COALESCE(SUM(output),0),COUNT(*)-COUNT(output),COALESCE(SUM(json_extract(json,'$.tokens.reasoning')),0),COUNT(*)-COUNT(json_extract(json,'$.tokens.reasoning')),COALESCE(SUM(json_extract(json,'$.tokens.cache_write')),0),COUNT(*)-COUNT(json_extract(json,'$.tokens.cache_write')),COALESCE(SUM(cost_nanousd),0),COALESCE(SUM(CASE WHEN cost_nanousd IS NULL THEN total ELSE 0 END),0),SUM(CASE WHEN cost_nanousd IS NULL THEN 1 ELSE 0 END),MIN(occurred_at),MAX(occurred_at)";
fn breakdown(row: &rusqlite::Row<'_>, base: usize) -> rusqlite::Result<UsageBreakdown> {
    let measure = |offset: usize| -> rusqlite::Result<TokenMeasure> {
        Ok(TokenMeasure {
            known: unsigned(row, base + offset)?,
            unknown_events: unsigned(row, base + offset + 1)?,
        })
    };
    Ok(UsageBreakdown {
        events: unsigned(row, base)?,
        total: measure(1)?,
        input: measure(3)?,
        cached: measure(5)?,
        output: measure(7)?,
        reasoning: measure(9)?,
        cache_write: measure(11)?,
        cost_nanousd: row.get(base + 13)?,
        unpriced_tokens: unsigned(row, base + 14)?,
        unpriced_events: row
            .get::<_, Option<i64>>(base + 15)?
            .unwrap_or(0)
            .try_into()
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(base + 15, -1))?,
        started_at: row.get(base + 16)?,
        ended_at: row.get(base + 17)?,
    })
}

impl Store {
    pub fn session_page(
        &self,
        request: &SessionPageRequest,
        timezone: Tz,
    ) -> Result<SessionPage, StoreError> {
        self.session_page_ordered(request, timezone, None)
    }
    /// Prioritize active sessions across the whole matching history, with bounded pages.
    /// Only the indexed five-minute range needs its JSON status inspected.
    pub fn session_page_with_activity(
        &self,
        request: &SessionPageRequest,
        timezone: Tz,
        now: chrono::DateTime<chrono::Utc>,
        connected_sources: &[String],
    ) -> Result<SessionPage, StoreError> {
        self.session_page_ordered(request, timezone, Some((now, connected_sources)))
    }
    fn session_page_ordered(
        &self,
        request: &SessionPageRequest,
        timezone: Tz,
        activity_context: Option<(chrono::DateTime<chrono::Utc>, &[String])>,
    ) -> Result<SessionPage, StoreError> {
        let filter = &request.filter;
        if filter
            .model
            .as_ref()
            .is_some_and(|m| m.is_empty() || m.len() > 256)
            || request.cursor.as_ref().is_some_and(|c| {
                c.id.is_empty()
                    || c.id.len() > 256
                    || c.activity.len() > 64
                    || c.as_of.as_ref().is_some_and(|value| value.len() > 64)
            })
            || [&filter.from_day, &filter.through_day]
                .into_iter()
                .flatten()
                .any(|s| s.len() != 10)
            || matches!(request.direction, PageDirection::Newer) && request.cursor.is_none()
        {
            return Err(StoreError::Query);
        }
        let (from, through) = match (&filter.from_day, &filter.through_day) {
            (Some(from), Some(through)) if from <= through => (
                day_boundary(from, timezone)?,
                day_boundary_next(through, timezone)?,
            ),
            (None, None) => (String::new(), "9999".into()),
            _ => return Err(StoreError::Query),
        };
        // SQLite builds the matching-ID set once from covering fact indexes. A correlated
        // model/time EXISTS would rescan the fact range separately for every session.
        let scoped = filter.model.is_some() || filter.from_day.is_some();
        let scope = if scoped {
            " AND s.id IN (SELECT session_id FROM usage_facts WHERE (?3 IS NULL OR model=?3) AND occurred_at>=?4 AND occurred_at<?5)"
        } else {
            " AND (?3 IS NULL) AND ?4<=?5"
        };
        let (op, order) = match request.direction {
            PageDirection::Older => ("<", "DESC"),
            PageDirection::Newer => (">", "ASC"),
        };
        let (activity, id) = request
            .cursor
            .as_ref()
            .map(|c| (c.activity.as_str(), c.id.as_str()))
            .unwrap_or(("9999", "~"));
        let mut as_of = None;
        let mut running_ids = std::collections::HashSet::new();
        let mut ids = if let Some((now, sources)) = activity_context {
            let captured = match request.cursor.as_ref() {
                Some(cursor) => chrono::DateTime::parse_from_rfc3339(
                    cursor.as_of.as_deref().ok_or(StoreError::Query)?,
                )
                .map_err(|_| StoreError::Query)?
                .with_timezone(&chrono::Utc),
                None => now,
            };
            if captured > now + chrono::Duration::seconds(5) {
                return Err(StoreError::Query);
            }
            let through_activity = super::canonical_activity(&captured.to_rfc3339());
            let from_activity =
                super::canonical_activity(&(captured - chrono::Duration::minutes(5)).to_rfc3339());
            as_of = Some(through_activity.clone());
            let rank = request
                .cursor
                .as_ref()
                .map_or(2_i64, |c| i64::from(c.running));
            let sql = format!(
                "WITH running_sessions AS MATERIALIZED (
                    SELECT s.id FROM sessions s WHERE s.last_activity>=?6 AND s.last_activity<=?7
                    AND json_extract(s.metadata,'$.status')='active'
                    AND EXISTS(SELECT 1 FROM source_sessions ss WHERE ss.session_id=s.id AND ss.source_id IN (SELECT value FROM json_each(?8)))
                ), ordered AS (
                    SELECT s.id,s.last_activity,s.id IN (SELECT id FROM running_sessions) AS running
                    FROM sessions s WHERE 1=1{scope}
                ) SELECT id,running FROM ordered WHERE (running,last_activity,id) {op} (?9,?1,?2)
                ORDER BY running {order},last_activity {order},id {order} LIMIT 11"
            );
            let mut query = self.connection.prepare(&sql)?;
            query
                .query_map(
                    params![
                        activity,
                        id,
                        filter.model,
                        from,
                        through,
                        from_activity,
                        through_activity,
                        serde_json::to_string(sources)?,
                        rank
                    ],
                    |r| {
                        let id = r.get::<_, String>(0)?;
                        if r.get::<_, bool>(1)? {
                            running_ids.insert(id.clone());
                        }
                        Ok(id)
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            let sql = format!(
                "SELECT s.id FROM sessions s WHERE (s.last_activity,s.id) {op} (?1,?2){scope} ORDER BY s.last_activity {order},s.id {order} LIMIT 11"
            );
            let mut query = self.connection.prepare(&sql)?;
            query
                .query_map(params![activity, id, filter.model, from, through], |r| {
                    r.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let extra = ids.len() > 10;
        ids.truncate(10);
        if matches!(request.direction, PageDirection::Newer) {
            ids.reverse();
        }
        let items = ids
            .iter()
            .map(|id| self.session_by_id(id)?.ok_or(StoreError::Query))
            .collect::<Result<Vec<_>, _>>()?;
        let cursor = |session: &RecentSession| SessionCursor {
            activity: session.meta.last_activity.clone(),
            id: session.meta.id.clone(),
            running: running_ids.contains(&session.meta.id),
            as_of: as_of.clone(),
        };
        let mut result = SessionPage {
            items,
            ..Default::default()
        };
        if let Some(last) = result.items.last()
            && (extra && matches!(request.direction, PageDirection::Older)
                || request.cursor.is_some() && matches!(request.direction, PageDirection::Newer))
        {
            result.older = Some(cursor(last));
        }
        if let Some(first) = result.items.first()
            && (extra && matches!(request.direction, PageDirection::Newer)
                || request.cursor.is_some() && matches!(request.direction, PageDirection::Older))
        {
            result.newer = Some(cursor(first));
        }
        Ok(result)
    }
    pub fn session_by_id(&self, id: &str) -> Result<Option<RecentSession>, StoreError> {
        let row=self.connection.query_row("SELECT metadata,COALESCE(SUM(m.total),0),COALESCE(SUM(m.cost_nanousd),0),COALESCE(SUM(m.unpriced_tokens),0) FROM sessions s LEFT JOIN session_model_usage m ON m.session_id=s.id WHERE s.id=?1 GROUP BY s.id",[id],|r|Ok((r.get::<_,String>(0)?,unsigned(r,1)?,r.get::<_,i64>(2)?,unsigned(r,3)?))).optional()?;
        let Some((json, total, cost, unpriced)) = row else {
            return Ok(None);
        };
        let mut meta: crate::domain::SessionMeta = serde_json::from_str(&json)?;
        let title: Option<Option<String>> = self
            .connection
            .query_row(
                "SELECT title FROM session_titles WHERE session_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(title) = title {
            meta.title = title;
        }
        let (events, unknown_totals) = self.connection.query_row(
            "SELECT COUNT(*),COUNT(*)-COUNT(total) FROM usage_facts WHERE session_id=?1",
            [id],
            |r| Ok((unsigned(r, 0)?, unsigned(r, 1)?)),
        )?;
        let mut model_query = self.connection.prepare("SELECT model FROM session_model_usage WHERE session_id=?1 ORDER BY total DESC,model LIMIT 50")?;
        let mut source_query = self.connection.prepare(
            "SELECT source_id FROM source_sessions WHERE session_id=?1 ORDER BY source_id",
        )?;
        Ok(Some(RecentSession {
            meta,
            models: model_query
                .query_map([id], |r| r.get(0))?
                .collect::<Result<_, _>>()?,
            sources: source_query
                .query_map([id], |r| r.get(0))?
                .collect::<Result<_, _>>()?,
            total,
            events,
            unknown_totals,
            cost_nanousd: cost,
            unpriced_tokens: unpriced,
            reference: self.reference_estimate("", "", Some(id))?,
        }))
    }
    pub fn session_detail(
        &self,
        request: &SessionDetailRequest,
    ) -> Result<Option<SessionDetail>, StoreError> {
        if request.id.is_empty()
            || request.id.len() > 256
            || [&request.models_after, &request.prices_after]
                .into_iter()
                .flatten()
                .any(|s| s.len() > 256)
        {
            return Err(StoreError::Query);
        }
        let Some(session) = self.session_by_id(&request.id)? else {
            return Ok(None);
        };
        let usage = self.connection.query_row(
            &format!("SELECT {BREAKDOWN} FROM usage_facts WHERE session_id=?1"),
            [&request.id],
            |r| breakdown(r, 0),
        )?;
        let mut query=self.connection.prepare(&format!("SELECT model,{BREAKDOWN} FROM usage_facts WHERE session_id=?1 AND model>?2 GROUP BY model ORDER BY model LIMIT 51"))?;
        let mut models = query
            .query_map(
                params![request.id, request.models_after.as_deref().unwrap_or("")],
                |r| {
                    Ok(SessionModel {
                        model: r.get(0)?,
                        usage: breakdown(r, 1)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let models_more = models.len() > 50;
        models.truncate(50);
        let models_next = models_more.then(|| models.last().unwrap().model.clone());
        static PRICES: std::sync::OnceLock<crate::pricing::PriceBook> = std::sync::OnceLock::new();
        let book = PRICES.get_or_init(|| crate::pricing::PriceBook::bundled().unwrap_or_default());
        let mut query=self.connection.prepare("SELECT json_extract(json,'$.price_version') AS version,COUNT(*),COALESCE(SUM(total),0),COALESCE(SUM(cost_nanousd),0),COUNT(*)-COUNT(total) FROM usage_facts WHERE session_id=?1 AND (?2 IS NULL OR COALESCE(json_extract(json,'$.price_version'),'')>?2) GROUP BY version ORDER BY COALESCE(version,'') LIMIT 51")?;
        let mut prices = query
            .query_map(params![request.id, request.prices_after], |r| {
                let version: Option<String> = r.get(0)?;
                Ok(PriceCoverage {
                    reference: book
                        .prices
                        .iter()
                        .find(|p| Some(p.version.as_str()) == version.as_deref())
                        .cloned(),
                    version,
                    events: unsigned(r, 1)?,
                    tokens: unsigned(r, 2)?,
                    cost_nanousd: r.get(3)?,
                    unknown_totals: unsigned(r, 4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let prices_more = prices.len() > 50;
        prices.truncate(50);
        let prices_next =
            prices_more.then(|| prices.last().unwrap().version.clone().unwrap_or_default());
        Ok(Some(SessionDetail {
            session,
            usage,
            models,
            models_next,
            prices,
            prices_next,
        }))
    }
}
