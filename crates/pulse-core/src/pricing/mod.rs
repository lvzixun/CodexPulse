use crate::domain::UsageFact;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    pub version: String,
    pub provider: String,
    pub model: String,
    pub service_tier: String,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub max_input: Option<u64>,
    /// Integer micro-USD per million tokens. Fact costs are stored in nano-USD.
    pub input_microusd: u64,
    pub cached_microusd: u64,
    pub output_microusd: u64,
    pub source_url: String,
    pub checked_at: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PriceBook {
    pub prices: Vec<Price>,
}

impl PriceBook {
    pub fn price(&self, fact: &UsageFact) -> Option<(&Price, i64)> {
        fact.tokens.validate().ok()?;
        let input = fact.tokens.input?;
        let cached = fact.tokens.cached?;
        let output = fact.tokens.output?;
        if fact.tokens.cache_write.is_some_and(|n| n > 0) {
            return None;
        }
        let at = DateTime::parse_from_rfc3339(&fact.timestamp)
            .ok()?
            .with_timezone(&Utc);
        let tier = fact.service_tier.as_deref().unwrap_or("standard");
        let mut eligible = self
            .prices
            .iter()
            .filter(|p| {
                p.provider == fact.provider
                    && p.model == fact.model
                    && p.service_tier == tier
                    && p.max_input.is_none_or(|limit| input <= limit)
                    && DateTime::parse_from_rfc3339(&p.effective_from).is_ok_and(|t| at >= t)
                    && p.effective_to
                        .as_ref()
                        .is_none_or(|end| DateTime::parse_from_rfc3339(end).is_ok_and(|t| at < t))
            })
            .collect::<Vec<_>>();
        eligible.sort_by(|a, b| b.effective_from.cmp(&a.effective_from));
        let price = eligible.first()?;
        let numerator = (input.checked_sub(cached)? as u128) * price.input_microusd as u128
            + (cached as u128) * price.cached_microusd as u128
            + (output as u128) * price.output_microusd as u128;
        let nanos = i64::try_from((numerator + 500) / 1000).ok()?;
        Some((price, nanos))
    }
    pub fn apply(&self, fact: &mut UsageFact) {
        if let Some((price, cost)) = self.price(fact) {
            fact.price_version = Some(price.version.clone());
            fact.cost_nanousd = Some(cost);
        }
    }
}
