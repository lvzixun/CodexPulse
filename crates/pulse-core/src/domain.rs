use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("cached input exceeds input tokens")]
    InvalidCache,
    #[error("token count exceeds supported integer range")]
    Overflow,
    #[error("total tokens contradict input + output")]
    ConflictingTotal,
}

/// Missing counters remain unknown. Cache and reasoning are subsets, not extra totals.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: Option<u64>,
    pub cached: Option<u64>,
    pub output: Option<u64>,
    pub reasoning: Option<u64>,
    pub cache_write: Option<u64>,
    pub total: Option<u64>,
}

impl TokenCounts {
    pub fn validate(&self) -> Result<(), DataError> {
        for value in [
            self.input,
            self.cached,
            self.output,
            self.reasoning,
            self.cache_write,
            self.total,
        ]
        .into_iter()
        .flatten()
        {
            if value > i64::MAX as u64 {
                return Err(DataError::Overflow);
            }
        }
        if let (Some(input), Some(cached)) = (self.input, self.cached)
            && cached > input
        {
            return Err(DataError::InvalidCache);
        }
        if let (Some(input), Some(output)) = (self.input, self.output) {
            let total = input.checked_add(output).ok_or(DataError::Overflow)?;
            if total > i64::MAX as u64 {
                return Err(DataError::Overflow);
            }
            if self.total.is_some_and(|reported| reported != total) {
                return Err(DataError::ConflictingTotal);
            }
        }
        Ok(())
    }
    pub fn effective_total(&self) -> Option<u64> {
        match (self.input, self.output) {
            (Some(input), Some(output)) => input.checked_add(output),
            _ => self.total,
        }
    }
    pub fn is_zero(&self) -> bool {
        self.effective_total() == Some(0)
    }
    pub fn checked_delta(&self, previous: &Self) -> Option<Self> {
        fn delta(now: Option<u64>, prev: Option<u64>) -> Option<Option<u64>> {
            match (now, prev) {
                (Some(a), Some(b)) => a.checked_sub(b).map(Some),
                (None, None) => Some(None),
                // A field appearing/disappearing is unknown for this delta, rather than
                // evidence that every independently known counter has reset.
                _ => Some(None),
            }
        }
        let result = Self {
            input: delta(self.input, previous.input)?,
            cached: delta(self.cached, previous.cached)?,
            output: delta(self.output, previous.output)?,
            reasoning: delta(self.reasoning, previous.reasoning)?,
            cache_write: delta(self.cache_write, previous.cache_write)?,
            total: delta(self.total, previous.total)?,
        };
        result.validate().ok()?;
        Some(result)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMeta {
    pub id: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub source_kind: Option<String>,
    pub source_version: Option<String>,
    pub parent_id: Option<String>,
    pub status: String,
    pub last_activity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageFact {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub model: String,
    pub provider: String,
    pub timestamp: String,
    pub tokens: TokenCounts,
    pub service_tier: Option<String>,
    pub quality: String,
    pub price_version: Option<String>,
    pub cost_nanousd: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseIssue {
    pub code: String,
    pub timestamp: Option<String>,
}
