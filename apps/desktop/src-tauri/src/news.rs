use pulse_core::news::{NewsItem, history_items, merge, status_items};
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
struct FeedUpdate {
    status: Cache,
    history: Cache,
    items: Vec<NewsItem>,
    wait: i64,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NewsSnapshot {
    pub items: Vec<NewsItem>,
    pub status: String,
    pub last_success: Option<String>,
    pub last_attempt: Option<String>,
}
impl Feed {
    pub fn for_storage(&self) -> Self {
        let mut feed = self.clone();
        if feed.status_cache.no_store || feed.history_cache.no_store {
            feed.items.clear();
            feed.status_cache = Cache::default();
            feed.history_cache = Cache::default();
            feed.last_success = None;
            feed.status = "awaiting_refresh".into();
        }
        feed
    }
    pub fn view(&self) -> NewsSnapshot {
        NewsSnapshot {
            items: self.items.clone(),
            status: self.status.clone(),
            last_success: self.last_success.clone(),
            last_attempt: self.last_attempt.clone(),
        }
    }
}
pub fn fetch(mut old: Feed) -> Feed {
    let now = chrono::Utc::now();
    old.last_attempt = Some(now.to_rfc3339());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let run = || -> Result<FeedUpdate, FetchError> {
        let (status, wait_a) = fetch_endpoint(
            &agent,
            "https://codex-resets.com/api/v1/status",
            &old.status_cache,
        )?;
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
            wait: wait_a.max(wait_b).max(300),
        })
    };
    match run() {
        Ok(update) => {
            old.status_cache = update.status;
            old.history_cache = update.history;
            old.items = update.items;
            old.status = "connected".into();
            old.last_success = Some(chrono::Utc::now().to_rfc3339());
            old.failures = 0;
            old.next_attempt = now.timestamp().saturating_add(update.wait);
        }
        Err((code, wait)) => {
            old.status = code;
            old.failures = old.failures.saturating_add(1);
            let backoff = (300u64.saturating_mul(1u64 << old.failures.min(5))).min(3600) as i64;
            old.next_attempt = now.timestamp().saturating_add(wait.max(backoff));
        }
    }
    old
}
fn fetch_endpoint(agent: &ureq::Agent, url: &str, old: &Cache) -> Result<(Cache, i64), FetchError> {
    let mut request = agent
        .get(url)
        .header("User-Agent", "CodexPulse/0.1.0 (+https://codex-resets.com)")
        .header("Accept", "application/json");
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
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v >= 0)
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
            let body = serde_json::from_slice(&bytes).map_err(|_| ("invalid_json".into(), 300))?;
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
