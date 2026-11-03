use crate::domain::UsageFact;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn cached_supported() -> bool {
    true
}

pub mod catalog;
mod estimate;
pub use estimate::Quote;

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
    #[serde(default = "cached_supported")]
    pub cached_supported: bool,
    #[serde(default)]
    pub cache_write_microusd: Option<u64>,
    #[serde(default)]
    pub cache_write_12h_microusd: Option<u64>,
    pub output_microusd: u64,
    pub source_url: String,
    pub checked_at: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PriceBook {
    pub prices: Vec<Price>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

impl PriceBook {
    pub fn bundled() -> Result<Self, serde_json::Error> {
        let mut book: Self = serde_json::from_str(include_str!(
            "../../../../resources/pricing/openai-2026-10-06.json"
        ))?;
        book.prices.extend(
            catalog::openai(
                include_str!("../../../../resources/pricing/openai-2026-11-03.md"),
                "2026-11-03",
            )
            .expect("validated bundled OpenAI prices"),
        );
        book.prices.extend(
            catalog::deepseek(
                include_str!("../../../../resources/pricing/deepseek-2026-11-03.html"),
                "2026-11-03",
            )
            .expect("validated bundled DeepSeek prices"),
        );
        Ok(book)
    }
    pub fn price(&self, fact: &UsageFact) -> Option<(&Price, i64)> {
        self.price_at(fact, &fact.timestamp)
    }
    /// API equivalence at the book's checked date, independent of historical billing.
    /// This never changes a fact's recorded price or fills unknown request dimensions.
    pub fn reference_price(&self, fact: &UsageFact) -> Option<(&Price, i64)> {
        let at = self.checked_at()?;
        self.price_at(fact, &format!("{at}T23:59:59Z"))
    }
    pub fn checked_at(&self) -> Option<&str> {
        self.prices.iter().map(|p| p.checked_at.as_str()).max()
    }
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        // Daily checks with unchanged rates must not restart the historical projection.
        let mut prices = self
            .prices
            .iter()
            .map(|p| {
                serde_json::json!([
                    p.provider,
                    p.model,
                    p.service_tier,
                    p.min_input,
                    p.max_input,
                    p.input_microusd,
                    p.cached_microusd,
                    p.cached_supported,
                    p.cache_write_microusd,
                    p.cache_write_12h_microusd,
                    p.output_microusd
                ])
            })
            .collect::<Vec<_>>();
        prices.sort_by_cached_key(|p| p.to_string());
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(2, prices, &self.aliases)).expect("price book"))
        )
    }
    pub fn replace_provider(&mut self, provider: &str, prices: Vec<Price>) {
        // Keep the original recorded-price metadata for historical detail views.
        self.prices
            .retain(|p| p.provider != provider || p.version.starts_with("openai-2026-10-06-"));
        self.prices.extend(prices);
    }
    pub fn set_alias(&mut self, alias: &str, model: &str) -> Result<(), &'static str> {
        let alias = alias.trim().to_ascii_lowercase();
        let model = model.trim().to_ascii_lowercase();
        if alias.is_empty()
            || alias.len() > 160
            || alias.chars().any(char::is_control)
            || self.aliases.len() >= 128 && !self.aliases.contains_key(&alias)
        {
            return Err("模型关联无效");
        }
        if model.is_empty() {
            self.aliases.remove(&alias);
        } else if alias != model && self.prices.iter().any(|p| p.model == model) {
            self.aliases.insert(alias, model);
        } else {
            return Err("参考模型暂不支持");
        }
        Ok(())
    }
    fn price_at(&self, fact: &UsageFact, timestamp: &str) -> Option<(&Price, i64)> {
        fact.tokens.validate().ok()?;
        let input = fact.tokens.input?;
        if fact.request_input.is_some_and(|request| request != input) {
            return None;
        }
        let cached = fact.tokens.cached?;
        let output = fact.tokens.output?;
        let writes = fact.tokens.cache_write?;
        let at = DateTime::parse_from_rfc3339(timestamp)
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
        if cached > 0 && !price.cached_supported {
            return None;
        }
        if writes > 0
            && price
                .cache_write_12h_microusd
                .is_some_and(|rate| Some(rate) != price.cache_write_microusd)
        {
            // The ledger has no write-lifetime dimension; only the reference
            // estimator can represent this uncertainty as a range.
            return None;
        }
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
