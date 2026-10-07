//! Public release metadata only: no account credentials or local usage are sent.
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{sync::Mutex, time::Duration};

pub const RELEASES_URL: &str = "https://github.com/lvzixun/CodexPulse/releases/latest";
const API_URL: &str = "https://api.github.com/repos/lvzixun/CodexPulse/releases/latest";
const DAY: i64 = 86400;

#[derive(Clone, Debug, Serialize)]
pub struct Info {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub checked_at: Option<String>,
    pub next_check_at: i64,
    pub status: String,
}
impl Default for Info {
    fn default() -> Self {
        Self {
            current_version: env!("CARGO_PKG_VERSION").into(),
            latest_version: None,
            update_available: false,
            checked_at: None,
            next_check_at: 0,
            status: "idle".into(),
        }
    }
}
#[derive(Default)]
struct Cache {
    info: Info,
    last_attempt: i64,
    blocked_until: i64,
}
impl Cache {
    fn due(&self, now: i64, manual: bool) -> bool {
        now >= self.blocked_until
            && (self.last_attempt == 0 || now >= self.last_attempt.saturating_add(60))
            && (manual || now >= self.info.next_check_at)
    }
    fn finish(&mut self, result: Result<Version, Failure>, now: i64) {
        self.last_attempt = now;
        self.info.checked_at = chrono::DateTime::from_timestamp(now, 0).map(|t| t.to_rfc3339());
        match result {
            Ok(latest) => {
                self.info.update_available = Version::parse(&self.info.current_version)
                    .is_ok_and(|current| latest.cmp_precedence(&current).is_gt());
                self.info.latest_version = Some(latest.to_string());
                self.info.status = if self.info.update_available {
                    "available"
                } else {
                    "current"
                }
                .into();
                self.info.next_check_at = now.saturating_add(DAY);
                self.blocked_until = 0;
            }
            Err(failure) => {
                self.info.status = failure.status.into();
                self.blocked_until = now.saturating_add(failure.wait.max(60));
                self.info.next_check_at = self.blocked_until;
            }
        }
    }
}
#[derive(Default)]
pub struct Service(Mutex<Cache>);
impl Service {
    // Called on a blocking worker, never the UI thread. Holding the lock makes
    // callers share a completed check; the one-minute floor merges manual clicks.
    pub fn check(&self, manual: bool) -> Result<Info, String> {
        let mut cache = self.0.lock().map_err(|_| "更新状态不可用")?;
        let now = chrono::Utc::now().timestamp();
        if cache.due(now, manual) {
            let result = fetch(API_URL);
            cache.finish(result, chrono::Utc::now().timestamp());
        }
        Ok(cache.info.clone())
    }
}
#[derive(Debug)]
struct Failure {
    status: &'static str,
    wait: i64,
}
fn failure(status: &'static str) -> Failure {
    Failure { status, wait: 900 }
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}
fn release_version(body: &str) -> Result<Version, Failure> {
    let release: Release = serde_json::from_str(body).map_err(|_| failure("invalid_response"))?;
    let version = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .map_err(|_| failure("invalid_response"))?;
    if release.draft || release.prerelease || !version.pre.is_empty() {
        return Err(failure("invalid_response"));
    }
    Ok(version)
}
fn fetch(url: &str) -> Result<Version, Failure> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header(
            "User-Agent",
            concat!("CodexPulse/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|_| failure("network_error"))?;
    let status = response.status().as_u16();
    if matches!(status, 403 | 429) {
        let now = chrono::Utc::now().timestamp();
        let after = response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .map(|v| crate::refresh::retry_after(Some(v), now))
            .unwrap_or(0);
        let reset = response
            .headers()
            .get("X-RateLimit-Reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok())
            .map(|at| at.saturating_sub(now))
            .unwrap_or(0);
        return Err(Failure {
            status: "rate_limited",
            wait: after.max(reset).max(900),
        });
    }
    if status != 200 {
        return Err(failure("unavailable"));
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(256 * 1024)
        .read_to_string()
        .map_err(|_| failure("invalid_response"))?;
    release_version(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(tag: &str) -> String {
        serde_json::json!({"tag_name":tag,"draft":false,"prerelease":false}).to_string()
    }
    #[test]
    fn compares_stable_versions_numerically() {
        let mut cache = Cache::default();
        cache.info.current_version = "0.1.9".into();
        cache.finish(release_version(&release("v0.1.10")), 1000);
        assert!(cache.info.update_available);
        assert_eq!(cache.info.latest_version.as_deref(), Some("0.1.10"));
        cache.finish(release_version(&release("v0.1.9")), 2000);
        assert!(!cache.info.update_available);
        cache.finish(release_version(&release("0.1.8")), 3000);
        assert!(!cache.info.update_available);
        cache.finish(release_version(&release("v0.1.9+build.2")), 4000);
        assert!(!cache.info.update_available);
    }
    #[test]
    fn rejects_untrusted_or_preview_metadata() {
        for body in [
            "{}".into(),
            release("latest"),
            release("v1.0.0-beta.1"),
            r#"{"tag_name":"v1.0.0","draft":true,"prerelease":false}"#.into(),
            r#"{"tag_name":"v1.0.0","draft":false,"prerelease":true}"#.into(),
        ] {
            assert!(release_version(&body).is_err());
        }
    }
    #[test]
    fn caches_checks_and_honors_retry_even_when_manual() {
        let mut cache = Cache::default();
        assert!(cache.due(1000, false));
        cache.finish(release_version(&release("v9.0.0")), 1000);
        assert!(!cache.due(1001, true));
        assert!(cache.due(1060, true));
        assert!(!cache.due(1060, false));
        assert!(cache.due(1000 + DAY, false));
        cache.finish(
            Err(Failure {
                status: "rate_limited",
                wait: 3600,
            }),
            2000,
        );
        assert!(!cache.due(5599, true));
        assert!(cache.due(5600, true));
        assert!(cache.info.update_available); // A failed check retains the known update.
    }
    #[test]
    fn http_transport_handles_success_and_retry_after() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        for (response, expected) in [
            (format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", release("v1.0.0").len(), release("v1.0.0")), true),
            ("HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1800\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(), false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut request = [0; 4096];
                let _ = socket.read(&mut request).unwrap();
                socket.write_all(response.as_bytes()).unwrap();
            });
            let result = fetch(&url);
            if expected { assert_eq!(result.unwrap(), Version::new(1, 0, 0)); }
            else { let error = result.unwrap_err(); assert_eq!(error.status, "rate_limited"); assert_eq!(error.wait, 1800); }
            server.join().unwrap();
        }
    }
}
