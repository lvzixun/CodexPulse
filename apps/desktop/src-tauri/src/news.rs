use pulse_core::news::{Challenge, NewsItem, challenge_page, history_items, merge, status_items};
use serde::{Deserialize, Serialize};
use std::{io::Read, time::Duration};

mod history;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Feed {
    pub items: Vec<NewsItem>,
    pub status: String,
    pub last_success: Option<String>,
    pub last_attempt: Option<String>,
    pub next_attempt: i64,
    pub failures: u32,
    pub status_cache: Cache,
    pub history_cache: Cache,
    #[serde(default)]
    pub history_limit: u32,
    #[serde(default)]
    pub history_pages: Vec<history::Page>,
    #[serde(default)]
    pub latest_reset: Option<NewsItem>,
    #[serde(default)]
    pub scheduled_reset: Option<NewsItem>,
    #[serde(default)]
    pub challenge_cache: Cache,
    #[serde(default)]
    pub challenge_status: String,
    #[serde(default)]
    pub challenge_next_attempt: i64,
    #[serde(default)]
    pub challenge_failures: u32,
    #[serde(default)]
    pub retry_until: i64,
    #[serde(default)]
    pub challenge_retry_until: i64,
    #[serde(default)]
    pub server_retry_until: i64,
    #[serde(default)]
    pub challenge_server_retry_until: i64,
    #[serde(default)]
    pub manual_retry_until: i64,
    #[serde(default)]
    pub status_cache_until: i64,
    #[serde(default)]
    pub challenge_cache_until: i64,
    #[serde(default)]
    pub request_status: String,
    #[serde(default)]
    pub display_next_attempt: Option<i64>,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Cache {
    pub etag: Option<String>,
    pub body: Option<serde_json::Value>,
    pub fetched_at: Option<String>,
    #[serde(default)]
    pub no_store: bool,
}
type FetchError = (String, i64);
#[derive(Default)]
pub struct Translator {
    cache: Cache,
    next_attempt: i64,
}
impl Translator {
    pub fn translate(&mut self, keys: &[String]) -> Result<String, String> {
        let now = chrono::Utc::now().timestamp();
        let mut failure = None;
        if now >= self.next_attempt {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .tls_config(crate::http_quota::tls_config())
                .timeout_global(Some(Duration::from_secs(10)))
                .http_status_as_error(false)
                .max_redirects(0)
                .build()
                .into();
            match fetch_resource(
                &agent,
                "https://codex-resets.com/zh-CN",
                &self.cache,
                Resource::Translation,
            ) {
                Ok((cache, wait)) => {
                    self.cache = cache;
                    self.next_attempt = now.saturating_add(wait.max(300));
                }
                Err((_, wait)) => {
                    failure = Some("中文译文暂时无法同步，请稍后重试");
                    self.next_attempt = now.saturating_add(wait.max(300));
                }
            }
        }
        let text = keys
            .iter()
            .find_map(|key| self.cache.body.as_ref()?.get(key)?.as_str())
            .map(str::to_owned);
        // no-store permits use for this request only; do not serve it on a later click.
        if self.cache.no_store {
            self.cache = Cache::default();
            self.next_attempt = 0;
        }
        text.ok_or_else(|| {
            failure
                .unwrap_or(if self.cache.body.is_none() {
                    "中文译文暂时无法同步，请稍后重试"
                } else {
                    "这条消息暂未提供中文译文，可查看原文"
                })
                .into()
        })
    }
}
struct FeedUpdate {
    history: Cache,
    history_pages: Vec<history::Page>,
    items: Vec<NewsItem>,
    wait: i64,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NewsSnapshot {
    pub request_status: String,
    pub next_attempt: Option<i64>,
    pub unread_keys: Vec<String>,
    pub important_unread: usize,
    pub items: Vec<NewsItem>,
    pub status: String,
    pub last_success: Option<String>,
    pub last_attempt: Option<String>,
    pub latest_reset: Option<NewsItem>,
    pub scheduled_reset: Option<NewsItem>,
    pub active_watch: Option<pulse_core::news::ResetWatch>,
    pub challenge: Option<Challenge>,
    pub challenge_status: String,
    pub challenge_fetched_at: Option<String>,
    pub reset_stats: pulse_core::news::ResetStats,
    pub reset_history: Vec<ResetDate>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetDate {
    pub occurred_at: String,
    pub reset_type: String,
}
impl Feed {
    fn accept_status(&mut self, cache: Cache) -> Result<(), FetchError> {
        let body = cache
            .body
            .as_ref()
            .ok_or(("missing_status_cache".into(), 300))?;
        let rows = status_items(body).map_err(|e| (e.into(), 300))?;
        self.latest_reset = rows
            .iter()
            .find(|item| Some(item.id.as_str()) == body["data"]["latest_reset"]["id"].as_str())
            .cloned();
        self.scheduled_reset = rows
            .into_iter()
            .find(|item| item.kind == pulse_core::news::NewsKind::Scheduled);
        self.status_cache = cache;
        Ok(())
    }
    pub fn prepare_manual_refresh(&mut self, now: i64) -> Option<i64> {
        // A click may retry transient failures before automatic exponential backoff.
        // Preserve explicit server waits, and conservatively retain old 429 records
        // whose separate Retry-After metadata was not stored by earlier versions.
        self.retry_until = self.server_retry_until.max(self.manual_retry_until).max(
            if self.status == "http_429" && self.server_retry_until == 0 {
                self.retry_until
            } else {
                0
            },
        );
        self.challenge_retry_until = self
            .challenge_server_retry_until
            .max(self.manual_retry_until)
            .max(
                if self.challenge_status == "http_429" && self.challenge_server_retry_until == 0 {
                    self.challenge_retry_until
                } else {
                    0
                },
            );
        let next = self.retry_until.min(self.challenge_retry_until);
        if next > now {
            Some(next)
        } else {
            // Coalesce repeated clicks while retaining independent endpoint waits.
            self.manual_retry_until = now.saturating_add(30);
            None
        }
    }
    pub fn for_storage(&self) -> Self {
        let mut feed = self.clone();
        if feed.status_cache.no_store || feed.history_cache.no_store {
            feed.items.clear();
            feed.status_cache = Cache::default();
            feed.history_cache = Cache::default();
            feed.history_pages.clear();
            feed.last_success = None;
            feed.latest_reset = None;
            feed.scheduled_reset = None;
            feed.status = "awaiting_refresh".into();
        }
        if feed.challenge_cache.no_store {
            feed.challenge_cache = Cache::default();
            feed.challenge_status = "awaiting_refresh".into();
        }
        feed
    }
    pub fn view(&self) -> NewsSnapshot {
        let calendar_since = chrono::Utc::now().timestamp() - 190 * 86400;
        NewsSnapshot {
            request_status: self.request_status.clone(),
            next_attempt: self.display_next_attempt,
            unread_keys: Vec::new(),
            important_unread: 0,
            items: self.items.clone(),
            status: self.status.clone(),
            last_success: self
                .last_success
                .clone()
                .into_iter()
                .chain(self.challenge_cache.fetched_at.clone())
                .max(),
            last_attempt: self.last_attempt.clone(),
            latest_reset: self.latest_reset.clone(),
            scheduled_reset: self.scheduled_reset.clone(),
            active_watch: self
                .status_cache
                .body
                .as_ref()
                .and_then(pulse_core::news::active_watch),
            challenge: self
                .challenge_cache
                .body
                .clone()
                .and_then(|v| serde_json::from_value(v).ok()),
            challenge_status: self.challenge_status.clone(),
            challenge_fetched_at: self.challenge_cache.fetched_at.clone(),
            reset_stats: pulse_core::news::reset_stats(
                self.status_cache
                    .body
                    .as_ref()
                    .unwrap_or(&serde_json::Value::Null),
                self.history_cache
                    .body
                    .as_ref()
                    .unwrap_or(&serde_json::Value::Null),
            ),
            reset_history: self
                .history_cache
                .body
                .as_ref()
                .and_then(|body| history_items(body).ok())
                .unwrap_or_default()
                .into_iter()
                .filter(|item| matches!(item.reset_type.as_deref(), Some("regular" | "banked")))
                .filter(|item| {
                    chrono::DateTime::parse_from_rfc3339(&item.occurred_at)
                        .is_ok_and(|at| at.timestamp() >= calendar_since)
                })
                .map(|item| ResetDate {
                    occurred_at: item.occurred_at,
                    reset_type: item.reset_type.unwrap(),
                })
                .collect(),
        }
    }
}
pub fn fetch(mut old: Feed, interval: u64, manual: bool, cancelled: &dyn Fn() -> bool) -> Feed {
    let now = chrono::Utc::now();
    if manual {
        // A queued retry reserves the next slot when it actually starts, not at
        // the earlier click; otherwise another click could immediately follow it.
        old.manual_retry_until = now.timestamp().saturating_add(30);
    }
    old.last_attempt = Some(now.to_rfc3339());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(crate::http_quota::tls_config())
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let mut fresh_status = None;
    let mut run = || -> Result<FeedUpdate, FetchError> {
        if cancelled() {
            return Err(("cancelled".into(), 0));
        }
        let (status, wait_a) = fetch_endpoint(
            &agent,
            "https://codex-resets.com/api/v1/status",
            &old.status_cache,
        )?;
        if cancelled() {
            return Err(("cancelled".into(), 0));
        }
        let status_rows = status_items(
            status
                .body
                .as_ref()
                .ok_or(("missing_status_cache".into(), 300))?,
        )
        .map_err(|e| (e.into(), 300))?;
        // Keep a validated status response even if the independent history fails.
        fresh_status = Some((status, wait_a));
        let empty_history = Cache::default();
        let (history, history_pages, wait_b) = history::fetch(
            &old.history_pages,
            if old.history_limit == 100 {
                &old.history_cache
            } else {
                &empty_history
            },
            cancelled,
            |url, cache, timeout| {
                fetch_resource_until(&agent, url, cache, Resource::Json, Some(timeout))
            },
        )?;
        let history_items = history_items(
            history
                .body
                .as_ref()
                .ok_or(("missing_history_cache".into(), 300))?,
        )
        .map_err(|e| (e.into(), 300))?;
        Ok(FeedUpdate {
            history,
            history_pages,
            items: merge(status_rows, history_items),
            wait: wait_a.max(wait_b),
        })
    };
    if now.timestamp() >= old.retry_until && (manual || now.timestamp() >= old.next_attempt) {
        let result = run();
        if !cancelled()
            && let Some((status, wait)) = fresh_status
        {
            old.accept_status(status)
                .expect("validated status response");
            old.status_cache_until = now.timestamp().saturating_add(wait);
        }
        match result {
            Ok(update) => {
                old.history_cache = update.history;
                old.history_pages = update.history_pages;
                old.history_limit = 100;
                old.items = update.items;
                old.status = "connected".into();
                old.last_success = Some(chrono::Utc::now().to_rfc3339());
                old.failures = 0;
                old.retry_until = 0;
                old.server_retry_until = 0;
                old.status_cache_until = now.timestamp().saturating_add(update.wait);
                old.next_attempt = now
                    .timestamp()
                    .saturating_add(update.wait.max(interval as i64));
            }
            Err((code, wait)) => {
                old.server_retry_until = server_retry_until(&code, wait, now.timestamp());
                old.status = code;
                old.failures = old.failures.saturating_add(1);
                let backoff = (300u64.saturating_mul(1u64 << old.failures.min(5))).min(3600) as i64;
                old.next_attempt = now.timestamp().saturating_add(wait.max(backoff));
                old.retry_until = old.next_attempt;
            }
        }
    }
    // A changed/failed challenge page must not discard working quota-reset data.
    if !cancelled()
        && now.timestamp() >= old.challenge_retry_until
        && (manual || now.timestamp() >= old.challenge_next_attempt)
    {
        match fetch_resource(
            &agent,
            "https://codex-resets.com/zh-CN/tibo-28",
            &old.challenge_cache,
            Resource::Challenge,
        ) {
            Ok((cache, wait)) => {
                old.challenge_cache = cache;
                old.challenge_status = "connected".into();
                old.challenge_failures = 0;
                old.challenge_retry_until = 0;
                old.challenge_server_retry_until = 0;
                old.challenge_cache_until = now.timestamp().saturating_add(wait);
                old.challenge_next_attempt =
                    now.timestamp().saturating_add(wait.max(interval as i64));
            }
            Err((code, wait)) => {
                old.challenge_server_retry_until = server_retry_until(&code, wait, now.timestamp());
                old.challenge_status = code;
                old.challenge_failures = old.challenge_failures.saturating_add(1);
                let backoff =
                    (300u64.saturating_mul(1u64 << old.challenge_failures.min(5))).min(3600) as i64;
                old.challenge_next_attempt = now.timestamp().saturating_add(wait.max(backoff));
                old.challenge_retry_until = old.challenge_next_attempt;
            }
        }
    }
    old
}
fn server_retry_until(code: &str, wait: i64, now: i64) -> i64 {
    if code.starts_with("http_") && (wait > 0 || code == "http_429") {
        now.saturating_add(if code == "http_429" {
            wait.max(300)
        } else {
            wait
        })
    } else {
        0
    }
}
fn fetch_endpoint(agent: &ureq::Agent, url: &str, old: &Cache) -> Result<(Cache, i64), FetchError> {
    fetch_resource(agent, url, old, Resource::Json)
}
#[derive(Clone, Copy)]
enum Resource {
    Json,
    Challenge,
    Translation,
}
fn fetch_resource(
    agent: &ureq::Agent,
    url: &str,
    old: &Cache,
    resource: Resource,
) -> Result<(Cache, i64), FetchError> {
    fetch_resource_until(agent, url, old, resource, None)
}
fn fetch_resource_until(
    agent: &ureq::Agent,
    url: &str,
    old: &Cache,
    resource: Resource,
    timeout: Option<Duration>,
) -> Result<(Cache, i64), FetchError> {
    let mut request = agent
        .get(url)
        .header(
            "User-Agent",
            concat!(
                "CodexPulse/",
                env!("CARGO_PKG_VERSION"),
                " (+https://codex-resets.com)"
            ),
        )
        .header(
            "Accept",
            if !matches!(resource, Resource::Json) {
                "text/html"
            } else {
                "application/json"
            },
        );
    if let Some(etag) = &old.etag
        && !old.no_store
        && old.body.is_some()
    {
        request = request.header("If-None-Match", etag);
    }
    if let Some(timeout) = timeout {
        request = request.config().timeout_global(Some(timeout)).build();
    }
    let mut response = request.call().map_err(|_| ("network_error".into(), 300))?;
    let retry = response
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .map(|v| crate::refresh::retry_after(Some(v), chrono::Utc::now().timestamp()))
        .unwrap_or(0);
    let cache_control = response
        .headers()
        .get("Cache-Control")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let no_store = cache_control
        .split(',')
        .any(|s| s.trim().eq_ignore_ascii_case("no-store"));
    let ttl = Some(cache_control)
        .and_then(|v| {
            v.split(',').map(str::trim).find_map(|s| {
                s.strip_prefix("max-age=")
                    .and_then(|s| s.trim_matches('"').parse::<i64>().ok())
            })
        })
        .filter(|v| *v >= 0)
        .unwrap_or(300);
    match response.status().as_u16() {
        304 if old.body.is_some() && !old.no_store => {
            let mut cached = old.clone();
            cached.fetched_at = Some(chrono::Utc::now().to_rfc3339());
            cached.no_store = no_store;
            Ok((cached, ttl))
        }
        200 => {
            let etag = response
                .headers()
                .get("ETag")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let mut bytes = Vec::new();
            response
                .body_mut()
                .as_reader()
                .take(512 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| ("body_read_error".into(), 300))?;
            if bytes.len() > 512 * 1024 {
                return Err(("body_too_large".into(), 300));
            }
            let body = if !matches!(resource, Resource::Json) {
                let html = std::str::from_utf8(&bytes)
                    .map_err(|_| ("invalid_challenge_encoding".into(), 300))?;
                match resource {
                    Resource::Challenge => serde_json::to_value(
                        challenge_page(html).map_err(|code| (code.into(), 300))?,
                    ),
                    Resource::Translation => serde_json::to_value(
                        pulse_core::news::translations_page(html)
                            .map_err(|code| (code.into(), 300))?,
                    ),
                    Resource::Json => unreachable!(),
                }
                .map_err(|_| ("invalid_page_data".into(), 300))?
            } else {
                serde_json::from_slice(&bytes).map_err(|_| ("invalid_json".into(), 300))?
            };
            Ok((
                Cache {
                    etag,
                    body: Some(body),
                    fetched_at: Some(chrono::Utc::now().to_rfc3339()),
                    no_store,
                },
                ttl,
            ))
        }
        status => Err((format!("http_{status}"), retry)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    #[ignore = "explicit public pagination probe; no credentials or message text"]
    fn live_history_cursor_advances() {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(crate::http_quota::tls_config())
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();
        let (head, _) = fetch_endpoint(
            &agent,
            "https://codex-resets.com/api/v1/resets?limit=1&order=desc",
            &Cache::default(),
        )
        .unwrap();
        let head = head.body.unwrap();
        assert_eq!(head["data"].as_array().unwrap().len(), 1);
        assert_eq!(head["pagination"]["has_more"], true);
        let cursor = head["pagination"]["next_cursor"].as_str().unwrap();
        assert!(cursor.len() <= 1024);
        let mut url = url::Url::parse("https://codex-resets.com/api/v1/resets").unwrap();
        url.query_pairs_mut()
            .append_pair("limit", "1")
            .append_pair("order", "desc")
            .append_pair("cursor", cursor);
        let (next, _) = fetch_endpoint(&agent, url.as_str(), &Cache::default()).unwrap();
        let next = next.body.unwrap();
        assert_eq!(next["data"].as_array().unwrap().len(), 1);
        assert_ne!(head["data"][0]["id"], next["data"][0]["id"]);
        assert!(
            head["data"][0]["announced_at"].as_str().unwrap()
                > next["data"][0]["announced_at"].as_str().unwrap()
        );
        println!("public cursor advanced to a different older record; no body or cursor printed");
    }

    #[test]
    #[ignore = "explicit read-only public feed probe; reports phases and pagination without credentials or message text"]
    fn measure_live_public_feed_phases() {
        fn category(error: &ureq::Error) -> &'static str {
            match error {
                ureq::Error::Timeout(_) => "timeout",
                ureq::Error::Protocol(_) => "protocol",
                ureq::Error::Io(_) => "io",
                ureq::Error::HostNotFound => "dns",
                ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "tls",
                _ => "other",
            }
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(crate::http_quota::tls_config())
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();
        for (name, url) in [
            ("status", "https://codex-resets.com/api/v1/status"),
            (
                "history",
                "https://codex-resets.com/api/v1/resets?limit=100&order=desc",
            ),
        ] {
            let started = std::time::Instant::now();
            let result = agent
                .get(url)
                .header(
                    "User-Agent",
                    concat!(
                        "CodexPulse/",
                        env!("CARGO_PKG_VERSION"),
                        " (+https://codex-resets.com)"
                    ),
                )
                .header("Accept", "application/json")
                .call();
            let mut report = serde_json::json!({"resource": name});
            match result {
                Err(error) => {
                    report["phase"] = "request".into();
                    report["error_category"] = category(&error).into();
                }
                Ok(mut response) => {
                    report["http_status"] = response.status().as_u16().into();
                    report["headers_ms"] = (started.elapsed().as_millis() as u64).into();
                    let mut bytes = Vec::new();
                    match response
                        .body_mut()
                        .as_reader()
                        .take(512 * 1024 + 1)
                        .read_to_end(&mut bytes)
                    {
                        Ok(_) => {
                            report["phase"] = "complete".into();
                            report["body_bytes"] = bytes.len().into();
                            report["valid_bounded_json"] = (bytes.len() <= 512 * 1024
                                && serde_json::from_slice::<serde_json::Value>(&bytes).is_ok())
                            .into();
                            if bytes.len() <= 512 * 1024
                                && let Ok(value) =
                                    serde_json::from_slice::<serde_json::Value>(&bytes)
                            {
                                report["pagination"] = serde_json::json!({
                                    "has_more": value["pagination"]["has_more"].as_bool(),
                                    "has_cursor": value["pagination"]["next_cursor"].is_string(),
                                });
                                report["rows"] = value["data"].as_array().map(Vec::len).into();
                            }
                        }
                        Err(error) => {
                            report["phase"] = "body".into();
                            report["io_kind"] = format!("{:?}", error.kind()).into();
                            report["error_category"] = error
                                .get_ref()
                                .and_then(|e| e.downcast_ref::<ureq::Error>())
                                .map(category)
                                .unwrap_or("other")
                                .into();
                            report["partial_bytes"] = bytes.len().into();
                        }
                    }
                }
            }
            report["total_ms"] = (started.elapsed().as_millis() as u64).into();
            println!("{report}");
        }
    }

    #[test]
    fn calendar_projection_is_separate_from_the_hundred_message_limit() {
        let at = chrono::Utc::now().to_rfc3339();
        let data = (0..110)
            .map(|id| {
                serde_json::json!({"id":id.to_string(),"reset_type":"regular",
            "announced_at":at,"text":"Synthetic public text must not enter calendar DTO",
            "source":{"type":"observed"}})
            })
            .collect::<Vec<_>>();
        let history = serde_json::json!({"meta":{"api_version":"v1"},"data":data,"pagination":{"has_more":false}});
        let feed = Feed {
            items: merge(vec![], history_items(&history).unwrap()),
            history_cache: Cache {
                body: Some(history),
                ..Cache::default()
            },
            ..Feed::default()
        };
        let view = feed.view();
        assert_eq!(view.items.len(), 100);
        assert_eq!(view.reset_history.len(), 110);
        assert!(view.reset_stats.history_complete);
        let dates = serde_json::to_value(view.reset_history).unwrap();
        assert_eq!(dates[0].as_object().unwrap().len(), 2);
        assert!(!dates.to_string().contains("Synthetic public text"));
    }

    #[test]
    #[ignore = "explicit bounded public history traversal; no credentials, cursors or message text"]
    fn live_complete_history_uses_cursor_reader() {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(crate::http_quota::tls_config())
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();
        let (aggregate, pages, _) =
            history::fetch(&[], &Cache::default(), &|| false, |url, cache, timeout| {
                fetch_resource_until(&agent, url, cache, Resource::Json, Some(timeout))
            })
            .unwrap();
        let stats = pulse_core::news::reset_stats(
            &serde_json::Value::Null,
            aggregate.body.as_ref().unwrap(),
        );
        assert!(stats.history_complete);
        assert!(stats.total.is_some_and(|n| n > 1));
        assert!(stats.longest_wait_days.is_some());
        println!(
            "{}",
            serde_json::json!({"pages":pages.len(),"rows":stats.total,"longest_wait_days":stats.longest_wait_days})
        );
    }

    fn serve(response: String) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/feed", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                request.push_str(&line);
            }
            let _ = stream.write_all(response.as_bytes());
            request
        });
        (url, handle)
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .tls_config(crate::http_quota::tls_config())
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(3)))
            .build()
            .into()
    }

    #[test]
    fn fresh_status_survives_history_failure_without_clearing_backoff_or_history() {
        let mut feed = Feed {
            status: "network_error".into(),
            failures: 2,
            retry_until: 1234,
            last_success: Some("previous-success".into()),
            history_cache: Cache {
                body: Some(serde_json::json!({"old_history": true})),
                ..Default::default()
            },
            ..Default::default()
        };
        let cache = Cache {
            body: Some(serde_json::json!({"meta":{"api_version":"v1"},"data":{
                "latest_reset":{"id":"reset","reset_type":"regular","announced_at":"2026-10-02T21:18:48Z","text":"Reset confirmed","source":{"type":"observed"}},
                "scheduled_reset":null,
                "active_watch":{"level":"elevated","observed_at":"2026-10-06T22:00:00Z","expires_at":"2026-10-07T07:00:00Z","text":"New clue","source":{"type":"x_post","author":"thsottiaux"}}
            }})),
            ..Default::default()
        };
        feed.accept_status(cache.clone()).unwrap();
        assert!(feed.view().active_watch.is_some());
        assert_eq!(feed.latest_reset.as_ref().unwrap().id, "reset");
        assert_eq!(feed.status, "network_error");
        assert_eq!(feed.retry_until, 1234);
        assert_eq!(feed.failures, 2);
        assert_eq!(feed.last_success.as_deref(), Some("previous-success"));
        assert_eq!(
            feed.history_cache.body.as_ref().unwrap()["old_history"],
            true
        );
        assert!(
            feed.accept_status(Cache {
                body: Some(serde_json::json!({"invalid":true})),
                ..Default::default()
            })
            .is_err()
        );
        assert_eq!(feed.status_cache.body, cache.body);
    }

    #[test]
    fn watch_snapshot_uses_status_cache_independently_and_respects_no_store() {
        let mut feed = Feed {
            status_cache: Cache {
                body: Some(
                    serde_json::json!({"meta":{"api_version":"v1"},"data":{"active_watch":{"level":"elevated","observed_at":"2026-10-06T22:00:00Z","expires_at":"2026-10-07T07:00:00Z","text":"Synthetic clue","source":{"type":"x_post","author":"thsottiaux"}}}}),
                ),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(feed.items.is_empty());
        assert!(feed.view().active_watch.is_some());
        feed.status_cache.no_store = true;
        assert!(feed.view().active_watch.is_some());
        assert!(feed.for_storage().view().active_watch.is_none());
    }

    #[test]
    fn manual_refresh_retries_client_backoff_and_coalesces_repeated_clicks() {
        let mut feed = Feed {
            status: "http_403".into(),
            challenge_status: "network_error".into(),
            retry_until: 3600,
            challenge_retry_until: 3600,
            last_success: Some("2026-10-06T07:00:00Z".into()),
            ..Feed::default()
        };
        assert_eq!(feed.prepare_manual_refresh(100), None);
        assert_eq!((feed.retry_until, feed.challenge_retry_until), (0, 0));
        assert_eq!(feed.prepare_manual_refresh(101), Some(130));
        assert_eq!(feed.prepare_manual_refresh(130), None);
        assert_eq!(feed.last_success.as_deref(), Some("2026-10-06T07:00:00Z"));
    }

    #[test]
    fn manual_refresh_preserves_server_waits_and_legacy_rate_limits() {
        let mut feed = Feed {
            server_retry_until: 200,
            challenge_server_retry_until: 300,
            retry_until: 3600,
            challenge_retry_until: 3600,
            ..Feed::default()
        };
        assert_eq!(feed.prepare_manual_refresh(100), Some(200));
        assert_eq!((feed.retry_until, feed.challenge_retry_until), (200, 300));
        // The challenge may be retried while the reset endpoint is still waiting.
        feed.challenge_server_retry_until = 0;
        assert_eq!(feed.prepare_manual_refresh(100), None);
        assert_eq!(feed.retry_until, 200);
        let mut legacy = Feed {
            status: "http_429".into(),
            challenge_status: "http_429".into(),
            retry_until: 500,
            challenge_retry_until: 600,
            ..Feed::default()
        };
        assert_eq!(legacy.prepare_manual_refresh(100), Some(500));
    }

    #[test]
    fn refusal_without_retry_after_is_distinct_from_server_wait() {
        for (header, expected) in [("", 0), ("Retry-After: 120\r\n", 120)] {
            let (url, server) = serve(format!(
                "HTTP/1.1 403 Forbidden\r\n{header}Content-Length: 0\r\nConnection: close\r\n\r\n"
            ));
            let (code, wait) =
                fetch_resource(&agent(), &url, &Cache::default(), Resource::Json).unwrap_err();
            server.join().unwrap();
            assert_eq!(code, "http_403");
            assert_eq!(wait, expected);
            assert_eq!(
                server_retry_until(&code, wait, 100),
                if wait == 0 { 0 } else { 220 }
            );
        }
        assert_eq!(server_retry_until("http_429", 0, 100), 400);
    }

    #[test]
    fn challenge_cache_contains_parsed_records_and_honors_no_store() {
        let page = r#"<aside data-challenge-clock data-start="2026-10-05" data-days="28"></aside><script>not_stored</script>"#;
        let (url, server) = serve(format!(
            "HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            page.len(),
            page
        ));
        let (cache, _) =
            fetch_resource(&agent(), &url, &Cache::default(), Resource::Challenge).unwrap();
        server.join().unwrap();
        assert!(cache.no_store);
        assert_eq!(cache.body.as_ref().unwrap()["days"], 28);
        assert!(
            !cache
                .body
                .as_ref()
                .unwrap()
                .to_string()
                .contains("not_stored")
        );
        let feed = Feed {
            challenge_cache: cache,
            challenge_status: "connected".into(),
            ..Feed::default()
        };
        assert!(feed.view().challenge.is_some());
        assert!(feed.for_storage().view().challenge.is_none());
    }

    #[test]
    fn unknown_challenge_markup_does_not_replace_valid_cache() {
        let body = "<html>Unknown page</html>";
        let (url, server) = serve(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ));
        let old = Cache {
            body: Some(serde_json::json!({"days":28})),
            ..Cache::default()
        };
        assert_eq!(
            fetch_resource(&agent(), &url, &old, Resource::Challenge)
                .unwrap_err()
                .0,
            "unsupported_challenge_page"
        );
        assert_eq!(old.body.as_ref().unwrap()["days"], 28);
        server.join().unwrap();
    }
    #[test]
    fn group_success_time_includes_independent_challenge_and_respects_no_store() {
        let feed = Feed {
            last_success: Some("2026-10-06T07:00:00Z".into()),
            challenge_cache: Cache {
                fetched_at: Some("2026-10-06T08:00:00Z".into()),
                no_store: true,
                ..Cache::default()
            },
            ..Feed::default()
        };
        assert_eq!(
            feed.view().last_success.as_deref(),
            Some("2026-10-06T08:00:00Z")
        );
        assert_eq!(
            feed.for_storage().view().last_success.as_deref(),
            Some("2026-10-06T07:00:00Z")
        );
    }

    #[test]
    fn revalidation_retains_body_and_sends_validator() {
        let old = Cache {
            etag: Some("\"v1\"".into()),
            body: Some(serde_json::json!({"api_version":"v1"})),
            ..Cache::default()
        };
        let (url, server) = serve("HTTP/1.1 304 Not Modified\r\nCache-Control: max-age=900\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        let (cache, ttl) = fetch_endpoint(&agent(), &url, &old).unwrap();
        assert_eq!(cache.body, old.body);
        assert_eq!(ttl, 900);
        assert!(cache.fetched_at.is_some());
        assert!(
            server
                .join()
                .unwrap()
                .to_lowercase()
                .contains("if-none-match: \"v1\"")
        );
    }

    #[test]
    fn rate_limit_preserves_retry_after() {
        let (url, server) = serve("HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1200\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        assert_eq!(
            fetch_endpoint(&agent(), &url, &Cache::default()).unwrap_err(),
            ("http_429".into(), 1200)
        );
        server.join().unwrap();
    }
    #[test]
    fn translation_http_cache_contains_only_text_and_revalidates() {
        let page = r#"<li class='log-item' data-tweet-id='123'><p class='log-item-text' data-role='tweet-display-text'>中文</p></li><script>ignored</script>"#;
        let (url, server) = serve(format!(
            "HTTP/1.1 200 OK\r\nETag: \"zh-1\"\r\nCache-Control: max-age=900\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            page.len(),
            page
        ));
        let (cache, ttl) =
            fetch_resource(&agent(), &url, &Cache::default(), Resource::Translation).unwrap();
        server.join().unwrap();
        assert_eq!(ttl, 900);
        assert_eq!(cache.body.as_ref().unwrap()["123"], "中文");
        assert!(!cache.body.as_ref().unwrap().to_string().contains("ignored"));
        let (url,server)=serve("HTTP/1.1 304 Not Modified\r\nCache-Control: max-age=900\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
        let (next, _) = fetch_resource(&agent(), &url, &cache, Resource::Translation).unwrap();
        assert_eq!(next.body, cache.body);
        assert!(
            server
                .join()
                .unwrap()
                .to_lowercase()
                .contains("if-none-match: \"zh-1\"")
        );
    }

    #[test]
    fn oversized_body_is_bounded() {
        let body = "x".repeat(512 * 1024 + 1);
        let (url, server) = serve(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ));
        assert_eq!(
            fetch_endpoint(&agent(), &url, &Cache::default())
                .unwrap_err()
                .0,
            "body_too_large"
        );
        server.join().unwrap();
    }

    #[test]
    fn no_store_is_not_persisted_or_revalidated() {
        let body = "{\"api_version\":\"v1\"}";
        let (url, server) = serve(format!(
            "HTTP/1.1 200 OK\r\nCache-Control: no-store\r\nETag: \"private\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ));
        let (cache, _) = fetch_endpoint(&agent(), &url, &Cache::default()).unwrap();
        server.join().unwrap();
        assert!(cache.no_store);
        assert!(cache.body.is_some());
        let feed = Feed {
            status_cache: cache.clone(),
            ..Feed::default()
        };
        assert!(feed.for_storage().status_cache.body.is_none());
        let (url, server) = serve(
            "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        );
        assert_eq!(
            fetch_endpoint(&agent(), &url, &cache).unwrap_err().0,
            "http_304"
        );
        assert!(
            !server
                .join()
                .unwrap()
                .to_lowercase()
                .contains("if-none-match")
        );
    }
}
