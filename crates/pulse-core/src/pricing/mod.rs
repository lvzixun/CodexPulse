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
    #[serde(default)]
    pub min_input: Option<u64>,
    pub max_input: Option<u64>,
    /// Integer micro-USD per million tokens. Fact costs are stored in nano-USD.
    pub input_microusd: u64,
    pub cached_microusd: u64,
    #[serde(default)]
    pub cache_write_microusd: Option<u64>,
    pub output_microusd: u64,
    pub source_url: String,
    pub checked_at: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PriceBook {
    pub prices: Vec<Price>,
}

impl PriceBook {
    pub fn bundled() -> Result<Self, serde_json::Error> {
        serde_json::from_str(include_str!(
            "../../../../resources/pricing/openai-2026-10-06.json"
        ))
    }
    pub fn price(&self, fact: &UsageFact) -> Option<(&Price, i64)> {
        fact.tokens.validate().ok()?;
        let input = fact.tokens.input?;
        if fact.request_input.is_some_and(|request| request != input) {
            return None;
        }
        let cached = fact.tokens.cached?;
        let output = fact.tokens.output?;
        let writes = fact.tokens.cache_write?;
        let at = DateTime::parse_from_rfc3339(&fact.timestamp)
            .ok()?
            .with_timezone(&Utc);
        let tier = match fact.service_tier.as_deref()? {
            "default" => "standard",
            "priority" => "fast",
            tier => tier,
        };
        let mut eligible = self
            .prices
            .iter()
            .filter(|p| {
                p.provider == fact.provider
                    && p.model == fact.model
                    && p.service_tier == tier
                    && p.min_input
                        .is_none_or(|limit| fact.request_input.is_some_and(|n| n >= limit))
                    && p.max_input
                        .is_none_or(|limit| fact.request_input.is_some_and(|n| n <= limit))
                    && DateTime::parse_from_rfc3339(&p.effective_from).is_ok_and(|t| at >= t)
                    && p.effective_to
                        .as_ref()
                        .is_none_or(|end| DateTime::parse_from_rfc3339(end).is_ok_and(|t| at < t))
            })
            .collect::<Vec<_>>();
        eligible.sort_by(|a, b| b.effective_from.cmp(&a.effective_from));
        let price = eligible.first()?;
        let write_rate = if writes == 0 {
            0
        } else {
            price.cache_write_microusd?
        };
        let numerator = (input.checked_sub(cached)?.checked_sub(writes)? as u128)
            * price.input_microusd as u128
            + (cached as u128) * price.cached_microusd as u128
            + (writes as u128) * write_rate as u128
            + (output as u128) * price.output_microusd as u128;
        let nanos = i64::try_from((numerator + 500) / 1000).ok()?;
        Some((price, nanos))
    }
    pub fn apply(&self, fact: &mut UsageFact) {
        // Only explicit revaluation may replace an already recorded historical price.
        if fact.cost_nanousd.is_some() || fact.price_version.is_some() {
            return;
        }
        if let Some((price, cost)) = self.price(fact) {
            fact.price_version = Some(price.version.clone());
            fact.cost_nanousd = Some(cost);
        }
    }
}
