use pulse_core::news::{Challenge, NewsItem, challenge_page, history_items, merge, status_items};
use serde::{Deserialize, Serialize};
use std::{io::Read, time::Duration};

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
    status: Cache,
    history: Cache,
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
    pub challenge: Option<Challenge>,
    pub challenge_status: String,
    pub challenge_fetched_at: Option<String>,
}
impl Feed {
    pub fn for_storage(&self) -> Self {
        let mut feed = self.clone();
        if feed.status_cache.no_store || feed.history_cache.no_store {
            feed.items.clear();
            feed.status_cache = Cache::default();
            feed.history_cache = Cache::default();
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
            challenge: self
                .challenge_cache
                .body
                .clone()
                .and_then(|v| serde_json::from_value(v).ok()),
            challenge_status: self.challenge_status.clone(),
            challenge_fetched_at: self.challenge_cache.fetched_at.clone(),
        }
    }
}
pub fn fetch(mut old: Feed, interval: u64, manual: bool, cancelled: &dyn Fn() -> bool) -> Feed {
    let now = chrono::Utc::now();
    old.last_attempt = Some(now.to_rfc3339());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let run = || -> Result<FeedUpdate, FetchError> {
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
        let (history, wait_b) = fetch_endpoint(
            &agent,
            "https://codex-resets.com/api/v1/resets?limit=20&order=desc",
            &old.history_cache,
        )?;
        let status_items = status_items(
            status
                .body
                .as_ref()
                .ok_or(("missing_status_cache".into(), 300))?,
        )
        .map_err(|e| (e.into(), 300))?;
        let history_items = history_items(
            history
                .body
                .as_ref()
                .ok_or(("missing_history_cache".into(), 300))?,
        )
        .map_err(|e| (e.into(), 300))?;
        Ok(FeedUpdate {
            status,
            history,
            items: merge(status_items, history_items),
            wait: wait_a.max(wait_b),
        })
    };
    if now.timestamp() >= old.retry_until && (manual || now.timestamp() >= old.next_attempt) {
        match run() {
            Ok(update) => {
                old.status_cache = update.status;
                old.history_cache = update.history;
                old.items = update.items;
                let status = old
                    .status_cache
                    .body
                    .as_ref()
                    .expect("validated status cache");
                let status_rows = status_items(status).expect("validated status items");
                old.latest_reset = status_rows
                    .iter()
                    .find(|i| Some(i.id.as_str()) == status["data"]["latest_reset"]["id"].as_str())
                    .cloned();
                old.scheduled_reset = status_rows
                    .into_iter()
                    .find(|i| i.kind == pulse_core::news::NewsKind::Scheduled);
                old.status = "connected".into();
                old.last_success = Some(chrono::Utc::now().to_rfc3339());
                old.failures = 0;
                old.retry_until = 0;
                old.status_cache_until = now.timestamp().saturating_add(update.wait);
                old.next_attempt = now
                    .timestamp()
                    .saturating_add(update.wait.max(interval as i64));
            }
            Err((code, wait)) => {
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
                old.challenge_cache_until = now.timestamp().saturating_add(wait);
                old.challenge_next_attempt =
                    now.timestamp().saturating_add(wait.max(interval as i64));
            }
            Err((code, wait)) => {
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
    let mut request = agent
        .get(url)
        .header("User-Agent", "CodexPulse/0.1.0 (+https://codex-resets.com)")
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
    let mut response = request.call().map_err(|_| ("network_error".into(), 300))?;
    let retry = response
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .map(|v| crate::refresh::retry_after(Some(v), chrono::Utc::now().timestamp()))
        .unwrap_or(300);
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
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(3)))
            .build()
            .into()
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
