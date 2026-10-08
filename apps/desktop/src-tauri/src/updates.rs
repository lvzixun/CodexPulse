//! Public release metadata only: no account credentials or local usage are sent.
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{sync::Mutex, time::Duration};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const RELEASES_URL: &str = "https://github.com/lvzixun/CodexPulse/releases/latest";
const API_URL: &str = "https://github.com/lvzixun/CodexPulse/releases/latest/download/latest.json";

#[derive(Clone, Debug, Serialize)]
pub struct Info {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub checked_at: Option<String>,
    pub next_check_at: i64,
    pub status: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub revision: u64,
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
            downloaded_bytes: 0,
            total_bytes: None,
            revision: 0,
        }
    }
}
#[derive(Default)]
struct Cache {
    info: Info,
    startup_checked: bool,
    last_attempt: i64,
    blocked_until: i64,
    prepared: Option<(Update, Vec<u8>)>,
}
impl Cache {
    fn due(&self, now: i64) -> bool {
        !matches!(
            self.info.status.as_str(),
            "downloading" | "ready" | "installing"
        ) && now >= self.blocked_until
            && (self.last_attempt == 0 || now >= self.last_attempt.saturating_add(60))
    }
    fn reserve_check(&mut self, now: i64, startup: bool) -> bool {
        if startup {
            if self.startup_checked {
                return false;
            }
            // Consume startup even on failure/wait. It must never become an
            // automatic retry, and a manual check racing startup already counts.
            self.startup_checked = true;
            if self.last_attempt != 0 {
                return false;
            }
        }
        self.due(now)
    }
    fn finish(&mut self, result: Result<Version, Failure>, now: i64) {
        self.info.revision += 1;
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
                self.info.next_check_at = now.saturating_add(60);
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
    pub fn info(&self) -> Result<Info, String> {
        self.0
            .lock()
            .map(|cache| cache.info.clone())
            .map_err(|_| "更新状态不可用".into())
    }

    pub fn startup(&self, app: &tauri::AppHandle) -> Result<Info, String> {
        self.run_check(true, app)
    }

    pub fn check(&self, app: &tauri::AppHandle) -> Result<Info, String> {
        self.run_check(false, app)
    }

    fn run_check(&self, startup: bool, app: &tauri::AppHandle) -> Result<Info, String> {
        self.check_using(startup, || {
            let routing = routing(app)?;
            fetch(API_URL, routing.as_ref())
        })
    }

    fn check_using(
        &self,
        startup: bool,
        fetch: impl FnOnce() -> Result<Version, Failure>,
    ) -> Result<Info, String> {
        let mut cache = self.0.lock().map_err(|_| "更新状态不可用")?;
        let now = chrono::Utc::now().timestamp();
        if cache.reserve_check(now, startup) {
            let result = fetch();
            cache.finish(result, chrono::Utc::now().timestamp());
        }
        Ok(cache.info.clone())
    }

    // Reserve before spawning so concurrent checks and recreated WebViews share one download.
    pub fn prepare(&self, app: &tauri::AppHandle) -> Result<Info, String> {
        let mut cache = self.0.lock().map_err(|_| "更新状态不可用")?;
        if cache.info.status == "available" {
            cache.info.status = "downloading".into();
            cache.info.revision += 1;
            cache.info.downloaded_bytes = 0;
            cache.info.total_bytes = None;
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let service = app.state::<Service>();
                if service.download(&app).await.is_err() {
                    service.publish(&app, |cache| {
                        cache.info.status = "download_error".into();
                        let now = chrono::Utc::now().timestamp();
                        cache.blocked_until = now.saturating_add(900);
                        cache.info.next_check_at = cache.blocked_until;
                    });
                }
            });
        }
        Ok(cache.info.clone())
    }

    fn publish(&self, app: &tauri::AppHandle, change: impl FnOnce(&mut Cache)) {
        if let Ok(mut cache) = self.0.lock() {
            change(&mut cache);
            cache.info.revision += 1;
            let _ = app.emit("app-update", cache.info.clone());
        }
    }

    async fn download(&self, app: &tauri::AppHandle) -> Result<(), ()> {
        let handle = app.clone();
        let routes = tauri::async_runtime::spawn_blocking(move || routing(&handle))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        let exit_app = app.clone();
        let updater = app
            .updater_builder()
            .configure_client(move |client| routed_client(client, routes.clone()))
            .timeout(Duration::from_secs(15))
            .on_before_exit(move || {
                // Windows updater exits directly after starting NSIS; flush collectors first.
                exit_app.state::<crate::backend::Backend>().shutdown();
                exit_app.cleanup_before_exit();
            });
        #[cfg(windows)]
        let updater = updater.installer_arg(
            install_directory_arg(&std::env::current_exe().map_err(|_| ())?).ok_or(())?,
        );
        let updater = updater.build().map_err(|_| ())?;
        let Some(mut update) = updater.check().await.map_err(|_| ())? else {
            self.publish(app, |cache| {
                cache.info.update_available = false;
                cache.info.status = "current".into();
            });
            return Ok(());
        };
        if !safe_download(&update.download_url) {
            return Err(());
        }
        update.timeout = Some(Duration::from_secs(600));
        self.publish(app, |cache| {
            cache.info.latest_version = Some(update.version.clone())
        });
        let mut downloaded = 0u64;
        let mut last = std::time::Instant::now();
        let bytes = update
            .download(
                |chunk, total| {
                    downloaded = downloaded.saturating_add(chunk as u64);
                    if last.elapsed() >= Duration::from_millis(250) {
                        self.publish(app, |cache| {
                            cache.info.downloaded_bytes = downloaded;
                            cache.info.total_bytes = total;
                        });
                        last = std::time::Instant::now();
                    }
                },
                || {},
            )
            .await
            .map_err(|_| ())?;
        // download() verifies the mandatory signature before exposing bytes to installation.
        self.publish(app, |cache| {
            cache.info.downloaded_bytes = bytes.len() as u64;
            cache.info.total_bytes = Some(bytes.len() as u64);
            cache.prepared = Some((update, bytes));
            cache.info.status = "ready".into();
        });
        Ok(())
    }

    pub fn install(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let prepared = {
            let mut cache = self.0.lock().map_err(|_| "更新状态不可用")?;
            if cache.info.status != "ready" {
                return Err("更新尚未准备完成".into());
            }
            let prepared = cache.prepared.take().ok_or("更新尚未准备完成")?;
            cache.info.status = "installing".into();
            cache.info.revision += 1;
            let _ = app.emit("app-update", cache.info.clone());
            prepared
        };
        if prepared.0.install(&prepared.1).is_err() {
            self.publish(app, |cache| {
                cache.prepared = Some(prepared);
                cache.info.status = "ready".into();
            });
            return Err("安装更新失败，请重试或前往发布页下载".into());
        }
        // Windows NSIS relaunches automatically; on macOS replacement returns here.
        #[cfg(not(windows))]
        app.restart();
        #[cfg(windows)]
        Ok(())
    }
}
fn safe_download(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url
            .path()
            .starts_with("/lvzixun/CodexPulse/releases/download/")
        && url.username().is_empty()
        && url.password().is_none()
}
#[cfg(windows)]
fn install_directory_arg(executable: &std::path::Path) -> Option<std::ffi::OsString> {
    // NSIS requires /D to be last and unquoted, including paths containing spaces.
    // Updater appends custom installer arguments after its /UPDATE, /R and /ARGS.
    let mut arg = std::ffi::OsString::from("/D=");
    arg.push(executable.parent()?);
    Some(arg)
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
    version: String,
}
fn release_version(body: &str) -> Result<Version, Failure> {
    let release: Release = serde_json::from_str(body).map_err(|_| failure("invalid_response"))?;
    let version = Version::parse(
        release
            .version
            .strip_prefix('v')
            .unwrap_or(&release.version),
    )
    .map_err(|_| failure("invalid_response"))?;
    if !version.pre.is_empty() {
        return Err(failure("invalid_response"));
    }
    Ok(version)
}
fn routing(
    app: &tauri::AppHandle,
) -> Result<Option<crate::http_quota::PublicProxyRouting>, Failure> {
    let scopes = app
        .state::<crate::backend::Backend>()
        .public_proxy_scopes()
        .map_err(|_| failure("proxy_config_unreadable"))?;
    crate::http_quota::public_proxy_routing(&scopes).map_err(|error| {
        failure(if error.code == "proxy_config_invalid" {
            "proxy_config_invalid"
        } else {
            "proxy_config_unreadable"
        })
    })
}
fn routed_client(
    client: reqwest::ClientBuilder,
    routing: Option<crate::http_quota::PublicProxyRouting>,
) -> reqwest::ClientBuilder {
    match routing {
        Some(routing) => client
            .no_proxy()
            .proxy(reqwest::Proxy::custom(move |url| routing.for_url(url))),
        None => client,
    }
}
fn fetch(
    url: &str,
    routing: Option<&crate::http_quota::PublicProxyRouting>,
) -> Result<Version, Failure> {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)));
    let config = if let Some(routing) = routing {
        config.proxy(routing.ureq_proxy())
    } else {
        config
    };
    let agent: ureq::Agent = config.build().into();
    let mut response = agent
        .get(url)
        .header(
            "User-Agent",
            concat!("CodexPulse/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/json")
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
    #[cfg(windows)]
    #[test]
    fn update_keeps_the_running_directory_even_with_spaces_and_unicode() {
        assert_eq!(
            install_directory_arg(std::path::Path::new(
                r"D:\我的应用\Codex Pulse\codexpulse.exe"
            )),
            Some(std::ffi::OsString::from(r"/D=D:\我的应用\Codex Pulse"))
        );
    }
    fn release(tag: &str) -> String {
        serde_json::json!({"version":tag}).to_string()
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
            r#"{"version":123}"#.into(),
        ] {
            assert!(release_version(&body).is_err());
        }
    }
    #[test]
    fn staged_update_never_downloads_twice_or_checks_while_installing() {
        let mut cache = Cache::default();
        for status in ["downloading", "ready", "installing"] {
            cache.info.status = status.into();
            assert!(!cache.due(999999));
        }
        for url in [
            "http://github.com/lvzixun/CodexPulse/releases/download/v1/a.exe",
            "https://github.com/other/repo/releases/download/v1/a.exe",
            "https://example.com/a.exe",
        ] {
            assert!(!safe_download(&url.parse().unwrap()));
        }
        assert!(safe_download(
            &"https://github.com/lvzixun/CodexPulse/releases/download/v1/a.exe"
                .parse()
                .unwrap()
        ));
    }
    #[test]
    fn caches_checks_and_honors_retry_even_when_manual() {
        let mut cache = Cache::default();
        assert!(cache.reserve_check(1000, true));
        cache.finish(release_version(&release("v9.0.0")), 1000);
        assert!(!cache.reserve_check(1001, false));
        assert!(cache.reserve_check(1060, false));
        assert!(!cache.reserve_check(1060, true));
        assert!(!cache.reserve_check(1000 + 86400 * 30, true));
        cache.finish(
            Err(Failure {
                status: "rate_limited",
                wait: 3600,
            }),
            2000,
        );
        assert!(!cache.reserve_check(5599, false));
        assert!(cache.reserve_check(5600, false));
        assert!(cache.info.update_available); // A failed check retains the known update.
    }
    #[test]
    fn startup_failure_does_not_schedule_automatic_retries() {
        let mut cache = Cache::default();
        assert!(cache.reserve_check(1000, true));
        cache.finish(
            Err(Failure {
                status: "rate_limited",
                wait: 3600,
            }),
            1000,
        );
        for now in [1001, 4600, 86400, 86400 * 30] {
            assert!(!cache.reserve_check(now, true));
        }
        assert!(!cache.reserve_check(4599, false));
        assert!(cache.reserve_check(4600, false));
    }
    #[test]
    fn manual_check_before_startup_is_not_repeated_and_cache_reads_are_passive() {
        let service = Service::default();
        {
            let mut cache = service.0.lock().unwrap();
            assert!(cache.reserve_check(1000, false));
            cache.finish(release_version(&release("v9.0.0")), 1000);
            assert!(!cache.reserve_check(2000, true));
        }
        for _ in 0..10 {
            let info = service.info().unwrap();
            assert_eq!(info.revision, 1);
            assert_eq!(info.latest_version.as_deref(), Some("9.0.0"));
        }
        let cache = service.0.lock().unwrap();
        assert_eq!(cache.last_attempt, 1000);
        assert_eq!(cache.info.status, "available");
    }
    #[test]
    fn concurrent_startup_and_manual_checks_share_one_request() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let service = Service::default();
        let calls = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for startup in [true, false, true, false] {
                let service = &service;
                let calls = &calls;
                scope.spawn(move || {
                    let info = service
                        .check_using(startup, || {
                            calls.fetch_add(1, Ordering::Relaxed);
                            std::thread::sleep(Duration::from_millis(20));
                            Ok(Version::new(9, 0, 0))
                        })
                        .unwrap();
                    assert_eq!(info.revision, 1);
                });
            }
        });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(service.info().unwrap().revision, 1);
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
            let result = fetch(&url, None);
            if expected { assert_eq!(result.unwrap(), Version::new(1, 0, 0)); }
            else { let error = result.unwrap_err(); assert_eq!(error.status, "rate_limited"); assert_eq!(error.wait, 1800); }
            server.join().unwrap();
        }
    }
    fn request(stream: &mut std::net::TcpStream) -> String {
        use std::io::BufRead;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = std::io::BufReader::new(stream);
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            headers.push_str(&line);
        }
        headers.to_ascii_lowercase()
    }
    fn accept(listener: &std::net::TcpListener) -> std::net::TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    return stream;
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("test server: {e}"),
            }
        }
    }
    fn routes(home: &std::path::Path, proxy: &str) -> crate::http_quota::PublicProxyRouting {
        std::fs::write(
            home.join(".env"),
            format!("HTTPS_PROXY={proxy}\nNO_PROXY=127.0.0.1\nOPENAI_API_KEY=must-not-send"),
        )
        .unwrap();
        std::fs::write(home.join("auth.json"), "not valid JSON").unwrap();
        crate::http_quota::public_proxy_routing(&[crate::http_quota::Scope {
            source_id: "test".into(),
            home: home.into(),
            wsl: None,
        }])
        .unwrap()
        .unwrap()
    }
    #[test]
    fn updater_client_routes_download_redirects_and_keeps_proxy_auth_off_direct_origin() {
        use std::{io::Write, net::TcpListener};
        let direct = TcpListener::bind("127.0.0.1:0").unwrap();
        let direct_url = format!("http://{}/asset", direct.local_addr().unwrap());
        let origin = std::thread::spawn(move || {
            let mut socket = accept(&direct);
            let headers = request(&mut socket);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsigned",
                )
                .unwrap();
            headers
        });
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_url = format!("http://user:pass@{}", proxy.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for destination in ["http://cdn.update.invalid/package".to_owned(), direct_url] {
                let mut socket = accept(&proxy);
                requests.push(request(&mut socket));
                socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
            }
            requests
        });
        let dir = tempfile::tempdir().unwrap();
        let routing = routes(dir.path(), &proxy_url);
        let _ = rustls::crypto::ring::default_provider().install_default();
        let body = tauri::async_runtime::block_on(async {
            routed_client(reqwest::Client::builder(), Some(routing))
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .get("http://updates.invalid/manifest")
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        });
        assert_eq!(body, "signed");
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("get http://updates.invalid/manifest "));
        assert!(requests[1].starts_with("get http://cdn.update.invalid/package "));
        assert!(requests[0].contains("proxy-authorization: basic dxnlcjpwyxnz"));
        for headers in requests {
            assert!(
                !headers
                    .lines()
                    .any(|line| line.starts_with("authorization:")
                        || line.starts_with("cookie:")
                        || line.starts_with("chatgpt-account-id:"))
            );
            assert!(!headers.contains("must-not-send"));
        }
        let headers = origin.join().unwrap();
        assert!(!headers.contains("authorization:"));
        assert!(!headers.contains("must-not-send"));
    }
    #[test]
    fn update_check_uses_codex_proxy_and_preserves_retry_after() {
        use std::{io::Write, net::TcpListener};
        for limited in [false, true] {
            let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
            let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let mut socket = accept(&proxy);
                let connect = request(&mut socket);
                assert!(connect.starts_with("connect updates.invalid:80 "));
                socket
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .unwrap();
                let headers = request(&mut socket);
                let response = if limited {
                    "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 3600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
                } else {
                    let body = release("v9.0.0");
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                socket.write_all(response.as_bytes()).unwrap();
                [connect, headers].join("\n")
            });
            let dir = tempfile::tempdir().unwrap();
            let routing = routes(dir.path(), &proxy_url);
            let result = fetch("http://updates.invalid/manifest", Some(&routing));
            if limited {
                let failure = result.unwrap_err();
                assert_eq!(failure.status, "rate_limited");
                assert_eq!(failure.wait, 3600);
            } else {
                assert_eq!(result.unwrap(), Version::new(9, 0, 0));
            }
            let headers = server.join().unwrap();
            assert!(!headers.contains("authorization:"));
            assert!(!headers.contains("must-not-send"));
        }
    }
    #[test]
    fn authenticated_https_proxy_is_selected_for_github_and_asset_hosts() {
        use std::{io::Write, net::TcpListener};
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_url = format!("http://user:pass@{}", proxy.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..2 {
                let mut socket = accept(&proxy);
                requests.push(request(&mut socket));
                socket.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            }
            requests
        });
        let dir = tempfile::tempdir().unwrap();
        let routing = routes(dir.path(), &proxy_url);
        let _ = rustls::crypto::ring::default_provider().install_default();
        tauri::async_runtime::block_on(async {
            let client = routed_client(reqwest::Client::builder(), Some(routing))
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();
            for host in ["github.com", "release-assets.githubusercontent.com"] {
                assert!(
                    client
                        .get(format!("https://{host}/package"))
                        .send()
                        .await
                        .is_err()
                );
            }
        });
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("connect github.com:443 "));
        assert!(requests[1].starts_with("connect release-assets.githubusercontent.com:443 "));
        for headers in requests {
            assert!(headers.contains("proxy-authorization: basic dxnlcjpwyxnz"));
            assert!(
                !headers
                    .lines()
                    .any(|line| line.starts_with("authorization:")
                        || line.starts_with("cookie:")
                        || line.starts_with("chatgpt-account-id:"))
            );
            assert!(!headers.contains("must-not-send"));
        }
    }
    #[test]
    #[ignore = "live public updater download through local Codex .env; run explicitly"]
    fn live_codex_proxy_download_verifies_public_update_signature() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let home = std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap(),
                )
                .join(".codex")
            });
        let routing = crate::http_quota::public_proxy_routing(&[crate::http_quota::Scope {
            source_id: "live-test".into(),
            home,
            wsl: None,
        }])
        .unwrap()
        .expect("requires an explicitly configured proxy");
        assert!(
            routing
                .for_url(&"https://github.com".parse().unwrap())
                .is_some()
        );
        let _ = rustls::crypto::ring::default_provider().install_default();
        let started = std::time::Instant::now();
        tauri::async_runtime::block_on(async {
            let client = routed_client(reqwest::Client::builder(), Some(routing))
                .user_agent(concat!("CodexPulse/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(90))
                .build()
                .unwrap();
            let manifest: serde_json::Value = client
                .get("https://github.com/lvzixun/CodexPulse/releases/download/v0.1.20/latest.json")
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(manifest["version"], "0.1.20");
            let platform = &manifest["platforms"]["darwin-aarch64"];
            let url: url::Url = platform["url"].as_str().unwrap().parse().unwrap();
            assert!(safe_download(&url));
            let bytes = client
                .get(url)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .bytes()
                .await
                .unwrap();
            let config: serde_json::Value =
                serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
            let key = STANDARD
                .decode(config["plugins"]["updater"]["pubkey"].as_str().unwrap())
                .unwrap();
            let key =
                minisign_verify::PublicKey::decode(std::str::from_utf8(&key).unwrap()).unwrap();
            let sig = STANDARD
                .decode(platform["signature"].as_str().unwrap())
                .unwrap();
            let sig =
                minisign_verify::Signature::decode(std::str::from_utf8(&sig).unwrap()).unwrap();
            key.verify(&bytes, &sig, true).unwrap();
            assert!(
                sig.trusted_comment()
                    .split_whitespace()
                    .any(|p| p == "version:0.1.20")
            );
            println!(
                "Public manifest + signed package via Codex proxy: {} bytes, {:.1}s",
                bytes.len(),
                started.elapsed().as_secs_f64()
            );
        });
    }
}
