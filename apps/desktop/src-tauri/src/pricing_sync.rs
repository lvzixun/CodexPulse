//! Public official prices only: bounded HTTPS reads, daily cache, independent provider fallback.
use crate::http_quota::{Scope, public_proxy, tls_config};
use pulse_core::pricing::{PriceBook, catalog};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SourceStatus {
    pub checked: i64,
    pub next: i64,
    pub error: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cache {
    pub book: PriceBook,
    pub sources: BTreeMap<String, SourceStatus>,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            book: PriceBook::bundled().unwrap_or_default(),
            sources: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Status {
    pub syncing: bool,
    pub error: bool,
    pub checked_at: Option<String>,
    pub aliases: BTreeMap<String, String>,
    pub models: Vec<String>,
    pub dates: BTreeMap<String, String>,
}
impl Cache {
    pub(crate) fn for_startup(mut self) -> Self {
        let bundled = Self::default();
        for (provider, date) in bundled.status(false).dates {
            let cached = || self.book.prices.iter().filter(|p| p.provider == provider);
            let cached_date = cached().map(|p| p.checked_at.as_str()).max();
            let prices = || {
                bundled
                    .book
                    .prices
                    .iter()
                    .filter(|p| p.provider == provider && p.checked_at == date)
            };
            let missing_schedule = cached_date == Some(date.as_str())
                && prices().any(|p| p.effective_to.is_some())
                && !cached().any(|p| p.checked_at == date && p.effective_to.is_some());
            if cached_date.is_none_or(|cached| cached < date.as_str()) || missing_schedule {
                self.book
                    .replace_provider(&provider, prices().cloned().collect());
            }
        }
        // A new process checks each provider again, keeping any failure backoff in force.
        for source in self.sources.values_mut().filter(|source| !source.error) {
            source.next = 0;
        }
        self
    }

    pub fn due(&self, now: i64) -> bool {
        ["openai", "deepseek"]
            .iter()
            .any(|p| self.sources.get(*p).is_none_or(|s| s.next <= now))
    }
    pub fn status(&self, syncing: bool) -> Status {
        let mut models = self
            .book
            .prices
            .iter()
            .map(|p| p.model.clone())
            .collect::<Vec<_>>();
        models.sort();
        models.dedup();
        Status {
            syncing,
            error: self.sources.values().any(|s| s.error),
            checked_at: self
                .sources
                .values()
                .map(|s| s.checked)
                .filter(|n| *n > 0)
                .min()
                .and_then(|at| chrono::DateTime::from_timestamp(at, 0))
                .map(|at| at.to_rfc3339()),
            aliases: self.book.aliases.clone(),
            models,
            dates: ["openai", "deepseek"]
                .into_iter()
                .filter_map(|provider| {
                    self.book
                        .prices
                        .iter()
                        .filter(|p| p.provider == provider)
                        .map(|p| p.checked_at.clone())
                        .max()
                        .map(|date| (provider.into(), date))
                })
                .collect(),
        }
    }
    pub fn refresh(&mut self, scopes: &[Scope], now: i64) {
        let date = chrono::DateTime::from_timestamp(now, 0)
            .unwrap_or_default()
            .format("%Y-%m-%d")
            .to_string();
        for (provider, url) in [
            ("openai", catalog::OPENAI_URL),
            ("deepseek", catalog::DEEPSEEK_URL),
        ] {
            if self.sources.get(provider).is_some_and(|s| s.next > now) {
                continue;
            }
            let result = fetch(url, scopes, now).and_then(|body| {
                if provider == "openai" {
                    catalog::openai(&body, &date)
                } else {
                    catalog::deepseek(&body, &date)
                }
                .map_err(|_| 3600)
            });
            self.apply(provider, result, now);
        }
    }
    fn apply(
        &mut self,
        provider: &str,
        result: Result<Vec<pulse_core::pricing::Price>, i64>,
        now: i64,
    ) {
        let source = self.sources.entry(provider.into()).or_default();
        match result {
            Ok(prices) => {
                self.book.replace_provider(provider, prices);
                source.checked = now;
                source.error = false;
                source.next = now.saturating_add(86400);
            }
            Err(wait) => {
                source.error = true;
                source.next = now.saturating_add(wait.max(3600));
            }
        }
    }
}
fn fetch(url: &str, scopes: &[Scope], now: i64) -> Result<String, i64> {
    let host = if url == catalog::OPENAI_URL {
        "developers.openai.com"
    } else {
        "api-docs.deepseek.com"
    };
    let proxy = public_proxy(scopes, host).map_err(|_| 3600)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls_config())
        .proxy(proxy)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .max_redirects(0)
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header(
            "User-Agent",
            concat!("CodexPulse/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|_| 3600)?;
    if response.status().as_u16() != 200 {
        return Err(response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .map(|v| crate::refresh::retry_after(Some(v), now))
            .unwrap_or(3600)
            .max(3600));
    }
    response
        .body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_to_string()
        .map_err(|_| 3600)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daily_cache_failure_backoff_and_aliases_survive_reopen() {
        let mut cache = Cache::default();
        assert!(cache.due(100));
        for provider in ["openai", "deepseek"] {
            cache.sources.insert(
                provider.into(),
                SourceStatus {
                    checked: 100,
                    next: 86500,
                    error: false,
                },
            );
        }
        cache
            .book
            .set_alias("mode-custom", "deepseek-flash")
            .unwrap();
        assert!(!cache.due(101));
        assert!(cache.due(86500));
        let cached: Cache = serde_json::from_str(&serde_json::to_string(&cache).unwrap()).unwrap();
        assert_eq!(cached.book.aliases["mode-custom"], "deepseek-flash");
        assert!(!cached.status(false).error);
        cache.sources.get_mut("deepseek").unwrap().error = true;
        assert!(cache.status(false).error);
        assert_eq!(cache.book.fingerprint(), cached.book.fingerprint());
    }
    #[test]
    fn reopening_an_old_cache_uses_newer_bundled_prices_and_preserves_other_settings() {
        let mut cache = Cache::default();
        let old_openai = include_str!("../../../../resources/pricing/openai-2026-11-03.md");
        let deepseek = include_str!("../../../../resources/pricing/deepseek-2026-11-03.html");
        cache
            .book
            .replace_provider("openai", catalog::openai(old_openai, "2026-11-17").unwrap());
        cache.book.replace_provider(
            "deepseek",
            catalog::deepseek(deepseek, "2026-11-19").unwrap(),
        );
        cache.book.set_alias("mode-custom", "gpt-6.1-sol").unwrap();
        cache.apply("openai", Err(172800), 100);

        let restarted = cache.for_startup();
        let status = restarted.status(false);
        assert!(status.models.contains(&"gpt-6.2-astra".into()));
        assert_eq!(status.dates["openai"], "2026-11-18");
        assert_eq!(status.dates["deepseek"], "2026-11-19");
        assert_eq!(status.aliases["mode-custom"], "gpt-6.1-sol");
        assert_eq!(restarted.sources["openai"].next, 172900);
        assert!(status.error);
    }

    #[test]
    fn failed_provider_keeps_good_prices_and_respects_retry_after() {
        let mut cache = Cache::default();
        let hash = cache.book.fingerprint();
        cache.apply("openai", Err(172800), 100);
        assert_eq!(cache.book.fingerprint(), hash);
        assert_eq!(cache.sources["openai"].next, 172900);
        assert!(cache.sources["openai"].error);
        let prices = catalog::deepseek(
            include_str!("../../../../resources/pricing/deepseek-2026-11-03.html"),
            "2026-11-03",
        )
        .unwrap();
        cache.apply("deepseek", Ok(prices), 100);
        assert_eq!(cache.sources["deepseek"].next, 86500);
        assert!(!cache.due(101));
        assert!(cache.status(false).error);
        assert_eq!(cache.book.fingerprint(), hash);
    }
    #[test]
    fn startup_rechecks_recent_success_without_shortening_retry_after() {
        let mut cache = Cache::default();
        cache.apply("openai", Err(172800), 100);
        let deepseek = cache
            .book
            .prices
            .iter()
            .filter(|price| price.provider == "deepseek")
            .cloned()
            .collect();
        cache.apply("deepseek", Ok(deepseek), 100);
        assert!(!cache.due(101));

        let stored: Cache = serde_json::from_str(&serde_json::to_string(&cache).unwrap()).unwrap();
        let restarted = stored.for_startup();
        assert!(restarted.due(101));
        assert_eq!(restarted.sources["openai"].next, 172900);
        assert_eq!(restarted.sources["deepseek"].next, 0);
        assert_eq!(restarted.sources["deepseek"].checked, 100);
        assert_eq!(cache.book.fingerprint(), restarted.book.fingerprint());
    }

    #[test]
    #[ignore = "explicit public official pricing-page network probe; no account credentials"]
    fn live_official_prices() {
        let now = chrono::Utc::now().timestamp();
        let mut cache = Cache::default();
        for provider in ["openai", "deepseek"] {
            cache.sources.insert(
                provider.into(),
                SourceStatus {
                    checked: now - 60,
                    next: now + 86340,
                    error: false,
                },
            );
        }
        let mut cache = cache.for_startup();
        cache.refresh(&[], now);
        for provider in ["openai", "deepseek"] {
            assert!(
                !cache.sources[provider].error,
                "official {provider} refresh failed"
            );
            assert_eq!(cache.sources[provider].checked, now);
            assert_eq!(cache.sources[provider].next, now + 86400);
            println!(
                "Verified live official {provider} catalog: {} rows",
                cache
                    .book
                    .prices
                    .iter()
                    .filter(|p| p.provider == provider)
                    .count()
            );
        }
    }
}
