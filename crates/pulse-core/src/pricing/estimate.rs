use super::{Price, PriceBook};
use crate::domain::UsageFact;

pub const ASSUMED_STANDARD: u32 = 1;
pub const CONTEXT_RANGE: u32 = 2;
pub const WRITE_RANGE: u32 = 4;
pub const CACHE_RANGE: u32 = 8;
pub const PEAK_RANGE: u32 = 16;
pub const MODEL_INFERRED: u32 = 32;

#[derive(Debug, Default)]
pub struct Quote {
    pub cost: Option<i64>,
    pub max_cost: Option<i64>,
    pub flags: u32,
    pub reason: &'static str,
}

impl PriceBook {
    /// Current official API equivalence, independent of gateway billing and recorded costs.
    /// Unknown request dimensions produce a labelled range rather than invented zeroes.
    pub fn quote(&self, fact: &UsageFact) -> Quote {
        let fail = |reason| Quote {
            reason,
            ..Default::default()
        };
        let original = fact.model.trim().to_ascii_lowercase();
        let raw = original
            .strip_prefix("openai/")
            .or_else(|| original.strip_prefix("deepseek/"))
            .unwrap_or(&original);
        let mut model = self
            .aliases
            .get(&original)
            .or_else(|| self.aliases.get(raw))
            .map(String::as_str)
            .unwrap_or(raw);
        model = match model {
            "deepseek-v4.1-flash"
            | "deepseek-v4.1-flash-1017"
            | "deepseek-v4-flash"
            | "deepseek-v4-flash-vision-exp" => "deepseek-flash",
            _ => model,
        };
        let matches = |name: &str| {
            name == model
                || model.strip_prefix(name).is_some_and(|tail| {
                    tail.len() == 11
                        && tail.starts_with('-')
                        && chrono::NaiveDate::parse_from_str(&tail[1..], "%Y-%m-%d").is_ok()
                })
        };
        let Some(provider) = self
            .prices
            .iter()
            .find(|p| matches(&p.model))
            .map(|p| p.provider.as_str())
        else {
            return fail(if raw.starts_with("mode-") {
                "unknown_model_id"
            } else {
                "model_price_missing"
            });
        };
        let Some(input) = fact.tokens.input else {
            return fail("token_details_missing");
        };
        let Some(output) = fact.tokens.output else {
            return fail("token_details_missing");
        };
        if fact.tokens.validate().is_err() {
            return fail("invalid_token_counts");
        }
        let mut flags = 0;
        if fact.provider != provider {
            flags |= MODEL_INFERRED;
        }
        let tier = match fact.service_tier.as_deref() {
            Some("default" | "standard") => "standard",
            Some("priority" | "fast") => "fast",
            None | Some("auto") => {
                if provider == "openai" {
                    flags |= ASSUMED_STANDARD;
                }
                "standard"
            }
            Some(tier) => tier,
        };
        let date = self
            .prices
            .iter()
            .filter(|p| p.provider == provider)
            .map(|p| p.checked_at.as_str())
            .max()
            .unwrap_or_default();
        let eligible = self
            .prices
            .iter()
            .filter(|p| {
                p.provider == provider
                    && matches(&p.model)
                    && p.checked_at == date
                    && (provider == "deepseek" || p.service_tier == tier)
                    && fact.request_input.is_none_or(|n| {
                        p.min_input.is_none_or(|min| n >= min)
                            && p.max_input.is_none_or(|max| n <= max)
                    })
            })
            .collect::<Vec<_>>();
        if eligible.is_empty() {
            return fail(
                if self
                    .prices
                    .iter()
                    .any(|p| p.provider == provider && matches(&p.model) && p.checked_at == date)
                {
                    "pricing_tier_missing"
                } else {
                    "model_price_missing"
                },
            );
        }
        if provider == "deepseek" {
            flags |= PEAK_RANGE;
        }
        if fact.request_input.is_none()
            && eligible
                .iter()
                .any(|p| p.min_input.is_some() || p.max_input.is_some())
        {
            flags |= CONTEXT_RANGE;
        }
        if fact.tokens.cached.is_none() {
            flags |= CACHE_RANGE;
        }
        if provider == "openai"
            && eligible.iter().any(|p| p.cache_write_microusd.is_some())
            && (fact.tokens.cache_write.is_none() || fact.tokens.cache_write.is_some_and(|n| n > 0))
        {
            flags |= WRITE_RANGE;
        }
        let mut min = i64::MAX;
        let mut max = 0;
        for price in eligible {
            let Some((low, high)) = bounds(
                price,
                input,
                output,
                fact.tokens.cached,
                if provider == "deepseek" {
                    Some(0)
                } else {
                    fact.tokens.cache_write
                },
            ) else {
                return fail("token_details_missing");
            };
            min = min.min(low);
            max = max.max(high);
        }
        Quote {
            cost: Some(min),
            max_cost: Some(max),
            flags,
            reason: "",
        }
    }
}

fn bounds(
    p: &Price,
    input: u64,
    output: u64,
    cached: Option<u64>,
    writes: Option<u64>,
) -> Option<(i64, i64)> {
    let known_write = writes.unwrap_or(0);
    if !p.cached_supported && cached.is_some_and(|n| n > 0) {
        return None;
    }
    let cache_options = [
        cached.unwrap_or(0),
        cached.unwrap_or(if p.cached_supported {
            input.checked_sub(known_write)?
        } else {
            0
        }),
    ];
    let mut min = i64::MAX;
    let mut max = 0;
    for cache in cache_options {
        let remaining = input.checked_sub(cache)?;
        let write_options = [
            known_write,
            writes.unwrap_or(if p.cache_write_microusd.is_some() {
                remaining
            } else {
                0
            }),
        ];
        for write in write_options {
            let ordinary = remaining.checked_sub(write)?;
            for rate in [
                p.cache_write_microusd,
                p.cache_write_12h_microusd.or(p.cache_write_microusd),
            ] {
                let write_rate = if write == 0 { 0 } else { rate? };
                let value = ordinary as u128 * p.input_microusd as u128
                    + cache as u128 * p.cached_microusd as u128
                    + write as u128 * write_rate as u128
                    + output as u128 * p.output_microusd as u128;
                let value = i64::try_from((value + 500) / 1000).ok()?;
                min = min.min(value);
                max = max.max(value);
            }
        }
    }
    Some((min, max))
}
