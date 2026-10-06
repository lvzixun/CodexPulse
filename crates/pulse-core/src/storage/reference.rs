use super::{Store, StoreError, unsigned};
use crate::{domain::UsageFact, pricing::PriceBook};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ReferenceEstimate {
    pub price_date: String,
    pub cost_nanousd: i64,
    pub unpriced_tokens: u64,
    pub unpriced_events: u64,
    pub pending: bool,
}

impl Store {
    pub(super) fn model_reference(
        &self,
        start: &str,
        end: &str,
        model: &str,
        watermark: i64,
    ) -> Result<ReferenceEstimate, StoreError> {
        self.scoped_reference(start, end, None, Some(model), watermark)
    }
    /// Restartable bounded projection. Every attempted row, including unknown prices,
    /// advances the checkpoint in the same transaction. Price changes restart it.
    pub fn step_reference_prices(
        &mut self,
        book: &PriceBook,
        limit: u32,
    ) -> Result<usize, StoreError> {
        self.reference_date = book.checked_at().unwrap_or_default().into();
        let version = book.fingerprint();
        let tx = self.connection.transaction()?;
        let prior = tx
            .query_row(
                "SELECT through_rowid FROM reference_price_state WHERE singleton=1 AND version=?1",
                [&version],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        let after = prior.unwrap_or(0);
        let rows = {
            let mut query = tx.prepare(
                "SELECT rowid,json,occurred_at,session_id,model,total FROM usage_facts WHERE rowid>?1 ORDER BY rowid LIMIT ?2",
            )?;
            query
                .query_map(params![after, limit.clamp(1, 1024)], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        if rows.is_empty() && prior.is_some() {
            self.reference_version = version;
            return Ok(0);
        }
        let mut through = after;
        for (rowid, json, at, session, model, total) in &rows {
            let fact: UsageFact = serde_json::from_str(json)?;
            let cost = book.reference_price(&fact).map(|(_, cost)| cost);
            tx.execute("INSERT INTO reference_prices VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(fact_id) DO UPDATE SET version=excluded.version,cost_nanousd=excluded.cost_nanousd,occurred_at=excluded.occurred_at,session_id=excluded.session_id,model=excluded.model,total=excluded.total,ledger_rowid=excluded.ledger_rowid",params![fact.id,version,cost,at,session,model,total,rowid])?;
            through = *rowid;
        }
        tx.execute("INSERT INTO reference_price_state VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET version=excluded.version,through_rowid=excluded.through_rowid",params![version,through])?;
        tx.commit()?;
        self.reference_version = version;
        Ok(rows.len())
    }
    pub(super) fn reference_estimate(
        &self,
        start: &str,
        end: &str,
        session: Option<&str>,
    ) -> Result<ReferenceEstimate, StoreError> {
        let watermark = self.connection.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM usage_facts",
            [],
            |r| r.get::<_, i64>(0),
        )?;
        self.scoped_reference(start, end, session, None, watermark)
    }
    fn scoped_reference(
        &self,
        start: &str,
        end: &str,
        session: Option<&str>,
        model: Option<&str>,
        watermark: i64,
    ) -> Result<ReferenceEstimate, StoreError> {
        let checkpoint = self
            .connection
            .query_row(
                "SELECT through_rowid FROM reference_price_state WHERE singleton=1 AND version=?1",
                [&self.reference_version],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        if let Some(model) = model {
            // Separate statement below preserves the model-specific covering index.
            let mut result = self.connection.query_row("SELECT COALESCE(SUM(cost_nanousd),0),COALESCE(SUM(CASE WHEN cost_nanousd IS NULL THEN total ELSE 0 END),0),COALESCE(SUM(CASE WHEN cost_nanousd IS NULL THEN 1 ELSE 0 END),0) FROM reference_prices WHERE version=?1 AND model=?2 AND occurred_at>=?3 AND occurred_at<?4 AND ledger_rowid<=?5",params![self.reference_version,model,start,end,watermark.min(checkpoint)],|r| self.read_reference(r))?;
            if watermark > checkpoint {
                let (tokens,events) = self.connection.query_row("SELECT COALESCE(SUM(total),0),COUNT(*) FROM usage_facts WHERE rowid>?1 AND rowid<=?2 AND model=?3 AND occurred_at>=?4 AND occurred_at<?5",params![checkpoint,watermark,model,start,end],|r| Ok((unsigned(r,0)?,unsigned(r,1)?)))?;
                self.add_pending(&mut result, tokens, events)?;
            }
            return Ok(result);
        }
        let scope = if session.is_some() {
            "session_id=?2"
        } else {
            "occurred_at>=?2 AND occurred_at<?3"
        };
        let sql = format!(
            "SELECT COALESCE(SUM(cost_nanousd),0),COALESCE(SUM(CASE WHEN cost_nanousd IS NULL THEN total ELSE 0 END),0),COALESCE(SUM(CASE WHEN cost_nanousd IS NULL THEN 1 ELSE 0 END),0) FROM reference_prices WHERE version=?1 AND {scope} AND ledger_rowid<=?4"
        );
        let mut result = if let Some(session) = session {
            self.connection.query_row(
                &sql,
                params![
                    self.reference_version,
                    session,
                    "",
                    watermark.min(checkpoint)
                ],
                |r| self.read_reference(r),
            )?
        } else {
            self.connection.query_row(
                &sql,
                params![
                    self.reference_version,
                    start,
                    end,
                    watermark.min(checkpoint)
                ],
                |r| self.read_reference(r),
            )?
        };
        if watermark > checkpoint {
            let (tokens, events) = if let Some(session) = session {
                self.connection.query_row("SELECT COALESCE(SUM(total),0),COUNT(*) FROM usage_facts WHERE session_id=?1 AND rowid>?2 AND rowid<=?3",params![session,checkpoint,watermark],|r| Ok((unsigned(r,0)?,unsigned(r,1)?)))?
            } else {
                self.connection.query_row("SELECT COALESCE(SUM(total),0),COUNT(*) FROM usage_facts WHERE occurred_at>=?1 AND occurred_at<?2 AND rowid>?3 AND rowid<=?4",params![start,end,checkpoint,watermark],|r| Ok((unsigned(r,0)?,unsigned(r,1)?)))?
            };
            self.add_pending(&mut result, tokens, events)?;
        }
        Ok(result)
    }
    fn read_reference(
        &self,
        row: &rusqlite::Row<'_>,
    ) -> Result<ReferenceEstimate, rusqlite::Error> {
        Ok(ReferenceEstimate {
            price_date: self.reference_date.clone(),
            cost_nanousd: row.get(0)?,
            unpriced_tokens: unsigned(row, 1)?,
            unpriced_events: unsigned(row, 2)?,
            pending: false,
        })
    }
    fn add_pending(
        &self,
        result: &mut ReferenceEstimate,
        tokens: u64,
        events: u64,
    ) -> Result<(), StoreError> {
        result.unpriced_tokens = result
            .unpriced_tokens
            .checked_add(tokens)
            .ok_or(crate::domain::DataError::Overflow)?;
        result.unpriced_events = result
            .unpriced_events
            .checked_add(events)
            .ok_or(crate::domain::DataError::Overflow)?;
        result.pending = events > 0;
        Ok(())
    }
}
