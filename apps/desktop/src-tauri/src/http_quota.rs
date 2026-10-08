//! Credential material lives only in this adapter. Never serialize/debug it, follow
//! authenticated redirects, write Codex's files, or launch an auth/CLI fallback.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pulse_core::quota::{QuotaBucket, QuotaWindow};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub const PROFILE_URL: &str = "https://chatgpt.com/backend-api/wham/profiles/me";
pub const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
pub const RESET_CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
#[derive(Clone, PartialEq, Eq)]
pub struct Scope {
    pub source_id: String,
    pub home: PathBuf,
    pub wsl: Option<crate::source_config::WslTarget>,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Stamp {
    pub identity: Option<String>,
    pub revision: String,
    pub status: String,
    pub proxy_source: String,
}
pub struct Credentials {
    token: String,
    account_id: Option<String>,
    identity_confirmed: bool,
    pub stamp: Stamp,
    proxy: Option<ureq::Proxy>,
    pub proxy_source: String,
}
#[derive(Debug)]
pub struct Failure {
    pub code: String,
    pub retry_after: i64,
}
impl From<&str> for Failure {
    fn from(code: &str) -> Self {
        Self {
            code: code.into(),
            retry_after: 0,
        }
    }
}
#[derive(Debug)]
pub struct Readout {
    pub buckets: Vec<QuotaBucket>,
    pub allowance: crate::account_allowance::AccountAllowance,
    pub allowance_retry_after: i64,
    pub proxy_source: String,
}
pub struct ResultSet {
    pub scope: Scope,
    pub stamp: Stamp,
    pub result: Result<Readout, Failure>,
    pub profile: Result<crate::account_profile::AccountProfile, Failure>,
}
fn bytes(path: &Path, optional: bool) -> Result<Vec<u8>, Failure> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("credentials_unreadable".into()),
    };
    let mut content = Vec::new();
    file.take(256 * 1024 + 1)
        .read_to_end(&mut content)
        .map_err(|_| Failure::from("credentials_unreadable"))?;
    if content.len() > 256 * 1024 {
        return Err("credential_file_too_large".into());
    }
    Ok(content)
}
fn claims(jwt: &str) -> Value {
    jwt.split('.')
        .nth(1)
        .and_then(|v| URL_SAFE_NO_PAD.decode(v).ok())
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or(Value::Null)
}
fn text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 32768 && !s.chars().any(char::is_control))
}
fn env_values(content: &[u8]) -> Result<BTreeMap<String, String>, Failure> {
    let contents =
        std::str::from_utf8(content).map_err(|_| Failure::from("proxy_config_invalid"))?;
    let mut map = BTreeMap::new();
    for line in contents.lines() {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if ![
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
        ]
        .contains(&key)
        {
            continue;
        }
        let value = value.trim();
        let value = if value.starts_with('"') || value.starts_with('\'') {
            let quote = value.as_bytes()[0] as char;
            let Some(end) = value[1..].find(quote) else {
                return Err("proxy_config_invalid".into());
            };
            &value[1..end + 1]
        } else {
            value.split(" #").next().unwrap_or(value).trim()
        };
        map.insert(key.into(), value.into());
    }
    Ok(map)
}
fn bypass(value: &str, host: &str) -> bool {
    value.split(',').any(|p| {
        let p = p.trim().to_ascii_lowercase();
        p == "*"
            || p == host
            || p.strip_prefix("*.")
                .or_else(|| p.strip_prefix('.'))
                .is_some_and(|suffix| host.ends_with(&format!(".{suffix}")))
    })
}
fn proxy_config(content: &[u8], windows: bool) -> Result<(Option<ureq::Proxy>, String), Failure> {
    proxy_config_for_host(content, windows, "chatgpt.com")
}
fn proxy_config_for_host(
    content: &[u8],
    windows: bool,
    host: &str,
) -> Result<(Option<ureq::Proxy>, String), Failure> {
    let env = env_values(content)?;
    let value = |keys: &[&str]| -> Option<(String, bool)> {
        keys.iter()
            .find_map(|key| env.get(*key).cloned())
            .map(|s| (s, true))
            .or_else(|| {
                windows
                    .then(|| keys.iter().find_map(|key| std::env::var(key).ok()))
                    .flatten()
                    .map(|s| (s, false))
            })
    };
    if value(&["NO_PROXY", "no_proxy"]).is_some_and(|(s, _)| bypass(&s, host)) {
        return Ok((None, "no_proxy".into()));
    }
    let Some((proxy, local)) = value(&[
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]) else {
        return Ok((None, "direct".into()));
    };
    if proxy.trim().is_empty() {
        return Ok((None, "direct".into()));
    }
    Ok((
        Some(ureq::Proxy::new(proxy.trim()).map_err(|_| Failure::from("proxy_config_invalid"))?),
        if local { "codex_env" } else { "process_env" }.into(),
    ))
}
// Routing for public updater traffic. Do not expose Debug/Serialize: proxy URLs
// can contain proxy credentials. Account auth/config files are never read here.
#[derive(Clone)]
pub struct PublicProxyRouting {
    proxy: Option<ureq::Proxy>,
    url: Option<url::Url>,
    no_proxy: String,
}
impl PublicProxyRouting {
    pub fn ureq_proxy(&self) -> Option<ureq::Proxy> {
        self.proxy.clone()
    }
    pub fn for_url(&self, target: &url::Url) -> Option<url::Url> {
        if target
            .host_str()
            .is_some_and(|host| bypass(&self.no_proxy, host))
        {
            None
        } else {
            self.url.clone()
        }
    }
}
pub fn public_proxy_routing(scopes: &[Scope]) -> Result<Option<PublicProxyRouting>, Failure> {
    let content = if let Some(scope) = scopes.first() {
        bytes(&scope.home.join(".env"), true)
            .map_err(|_| Failure::from("proxy_config_unreadable"))?
    } else {
        Vec::new()
    };
    let env = env_values(&content)?;
    let process = scopes.first().is_none_or(|scope| scope.wsl.is_none());
    let value = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| env.get(*key).cloned())
            .or_else(|| {
                process
                    .then(|| keys.iter().find_map(|key| std::env::var(key).ok()))
                    .flatten()
            })
    };
    let Some(raw) = value(&[
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]) else {
        // Preserve the updater's existing system/ambient proxy behavior.
        return Ok(None);
    };
    let no_proxy = value(&["NO_PROXY", "no_proxy"]).unwrap_or_default();
    if raw.trim().is_empty() {
        return Ok(Some(PublicProxyRouting {
            proxy: None,
            url: None,
            no_proxy,
        }));
    }
    let original =
        ureq::Proxy::new(raw.trim()).map_err(|_| Failure::from("proxy_config_invalid"))?;
    let mut url = url::Url::parse(&original.uri().to_string())
        .map_err(|_| Failure::from("proxy_config_invalid"))?;
    if url.scheme() == "socks" {
        url.set_scheme("socks5")
            .map_err(|_| Failure::from("proxy_config_invalid"))?;
    }
    let mut builder = ureq::Proxy::builder(original.protocol())
        .host(original.host())
        .port(original.port())
        .resolve_target(original.resolve_target());
    if let Some(user) = original.username() {
        builder = builder.username(user);
    }
    if let Some(password) = original.password() {
        builder = builder.password(password);
    }
    for host in no_proxy.split(',') {
        builder = builder.no_proxy(&host.trim().to_ascii_lowercase());
    }
    let proxy = builder
        .build()
        .map_err(|_| Failure::from("proxy_config_invalid"))?;
    Ok(Some(PublicProxyRouting {
        proxy: Some(proxy),
        url: Some(url),
        no_proxy,
    }))
}
// Public feeds reuse only routing configuration, never account credentials.
pub fn public_proxy(scopes: &[Scope], host: &str) -> Result<Option<ureq::Proxy>, Failure> {
    let Some(scope) = scopes.first() else {
        return Ok(ureq::Proxy::try_from_env());
    };
    let content = bytes(&scope.home.join(".env"), true)
        .map_err(|_| Failure::from("proxy_config_unreadable"))?;
    proxy_config_for_host(&content, scope.wsl.is_none(), host).map(|(proxy, _)| proxy)
}
pub fn load(scope: &Scope) -> Result<Credentials, Failure> {
    let config = bytes(&scope.home.join("config.toml"), true)?;
    let config: toml::Value = if config.is_empty() {
        toml::Value::Table(Default::default())
    } else {
        std::str::from_utf8(&config)
            .ok()
            .and_then(|s| toml::from_str(s).ok())
            .ok_or_else(|| Failure::from("credential_config_invalid"))?
    };
    match config
        .get("cli_auth_credentials_store")
        .and_then(toml::Value::as_str)
        .unwrap_or("file")
    {
        "file" => {}
        "auto" | "keyring" => return Err("credential_store_unsupported".into()),
        "ephemeral" => return Err("credentials_ephemeral".into()),
        _ => return Err("credential_config_invalid".into()),
    }
    // A custom backend must not have its bearer token sent to our fixed ChatGPT endpoint.
    if config
        .get("chatgpt_base_url")
        .and_then(toml::Value::as_str)
        .is_some_and(|s| s.trim_end_matches('/') != "https://chatgpt.com/backend-api")
    {
        return Err("custom_backend_unsupported".into());
    }
    let auth = bytes(&scope.home.join("auth.json"), false)?;
    let env = bytes(&scope.home.join(".env"), true)?;
    let (proxy, proxy_source) = proxy_config(&env, scope.wsl.is_none())?;
    let data: Value =
        serde_json::from_slice(&auth).map_err(|_| Failure::from("credentials_invalid"))?;
    if data["auth_mode"].as_str().is_some_and(|m| m != "chatgpt") {
        return Err("auth_mode_unsupported".into());
    }
    let token =
        text(&data["tokens"]["access_token"]).ok_or_else(|| Failure::from("not_signed_in"))?;
    let id_claims = claims(text(&data["tokens"]["id_token"]).unwrap_or(""));
    let access_claims = claims(token);
    let account_id = text(&data["tokens"]["account_id"])
        .or_else(|| text(&id_claims["https://api.openai.com/auth"]["chatgpt_account_id"]))
        .map(str::to_owned);
    let subject = text(&access_claims["sub"]).or_else(|| text(&id_claims["sub"]));
    if account_id.is_none() && subject.is_none() {
        return Err("account_identity_unavailable".into());
    }
    let identity_confirmed = subject.is_some() && account_id.is_some();
    let identity_material = if identity_confirmed {
        serde_json::to_vec(&(subject, &account_id)).unwrap()
    } else {
        // Unknown user/workspace must not merge Windows and WSL on a partial identity.
        serde_json::to_vec(&(&scope.source_id, subject, &account_id)).unwrap()
    };
    let identity = format!("{:x}", Sha256::digest(identity_material));
    let mut digest = Sha256::new();
    digest.update(&auth);
    digest.update(config.to_string().as_bytes());
    digest.update(&env);
    let expired = access_claims["exp"]
        .as_i64()
        .is_some_and(|exp| exp <= chrono::Utc::now().timestamp());
    Ok(Credentials {
        token: token.into(),
        account_id,
        identity_confirmed,
        proxy,
        proxy_source: proxy_source.clone(),
        stamp: Stamp {
            identity: Some(identity),
            revision: format!("{:x}", digest.finalize()),
            status: if expired {
                "credentials_expired"
            } else {
                "ready"
            }
            .into(),
            proxy_source: proxy_source.clone(),
        },
    })
}
pub fn probe(scope: &Scope) -> Stamp {
    match load(scope) {
        Ok(c) => c.stamp,
        Err(e) => Stamp {
            status: e.code,
            ..Stamp::default()
        },
    }
}
pub fn checked_fetch<T>(
    scope: &Scope,
    mut started: impl FnMut(&Stamp),
    fetcher: impl FnOnce(&Credentials) -> Result<T, Failure>,
) -> (Stamp, Result<T, Failure>) {
    match load(scope) {
        Ok(credentials) => {
            let initial = credentials.stamp.clone();
            started(&initial);
            let result = fetcher(&credentials);
            let current = probe(scope);
            if current != initial {
                (current, Err("credentials_changed".into()))
            } else {
                (initial, result)
            }
        }
        Err(error) => (
            Stamp {
                status: error.code.clone(),
                ..Default::default()
            },
            Err(error),
        ),
    }
}
#[cfg(test)]
pub fn fetch(scope: &Scope, credentials: &Credentials) -> Result<Readout, Failure> {
    fetch_cancellable(scope, credentials, &|| false)
}
pub fn fetch_cancellable(
    scope: &Scope,
    credentials: &Credentials,
    cancelled: &impl Fn() -> bool,
) -> Result<Readout, Failure> {
    if cancelled() {
        return Err("cancelled".into());
    }
    if credentials.stamp.status != "ready" {
        return Err(credentials.stamp.status.as_str().into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls_config())
        .proxy(credentials.proxy.clone())
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut result = fetch_with(&agent, USAGE_URL, scope, credentials)?;
    if probe(scope) != credentials.stamp {
        return Err("credentials_changed".into());
    }
    if cancelled() {
        result.allowance.expiration_status = "cancelled".into();
        return Ok(result);
    }
    match fetch_json(&agent, RESET_CREDITS_URL, credentials).and_then(|value| {
        result
            .allowance
            .apply_cards(&value, chrono::Utc::now())
            .map_err(Failure::from)
    }) {
        Ok(()) => {}
        Err(error) => {
            result.allowance.expiration_status = error.code;
            result.allowance_retry_after = error.retry_after;
        }
    }
    Ok(result)
}
fn fetch_with(
    agent: &ureq::Agent,
    url: &str,
    scope: &Scope,
    c: &Credentials,
) -> Result<Readout, Failure> {
    let response = fetch_json(agent, url, c)?;
    let mut buckets = normalize(
        &scope.source_id,
        c.stamp.identity.as_deref().unwrap_or(""),
        &response,
    )?;
    for bucket in &mut buckets {
        bucket.identity_confirmed = c.identity_confirmed;
    }
    Ok(Readout {
        buckets,
        allowance: crate::account_allowance::usage(&response),
        allowance_retry_after: 0,
        proxy_source: c.proxy_source.clone(),
    })
}
pub fn tls_config() -> ureq::tls::TlsConfig {
    #[cfg(target_os = "macos")]
    {
        ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build()
    }
    #[cfg(not(target_os = "macos"))]
    {
        ureq::tls::TlsConfig::default()
    }
}
pub fn fetch_profile(c: &Credentials) -> Result<crate::account_profile::AccountProfile, Failure> {
    if c.stamp.status != "ready" {
        return Err(c.stamp.status.as_str().into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls_config())
        .proxy(c.proxy.clone())
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    crate::account_profile::normalize(&fetch_json(&agent, PROFILE_URL, c)?)
}
fn fetch_json(agent: &ureq::Agent, url: &str, c: &Credentials) -> Result<Value, Failure> {
    let mut request = agent
        .get(url)
        .header("Authorization", &format!("Bearer {}", c.token))
        .header("Accept", "application/json")
        .header("User-Agent", "CodexPulse/0.1.0");
    if let Some(id) = &c.account_id {
        request = request.header("ChatGPT-Account-Id", id);
    }
    let mut response = request.call().map_err(|_| Failure::from("network_error"))?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(Failure {
            code: if status == 401 {
                "reauth_required".into()
            } else {
                format!("http_{status}")
            },
            retry_after: crate::refresh::retry_after(
                response
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok()),
                chrono::Utc::now().timestamp(),
            ),
        });
    }
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(512 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|_| Failure::from("body_read_error"))?;
    if body.len() > 512 * 1024 {
        return Err("body_too_large".into());
    }
    let response: Value =
        serde_json::from_slice(&body).map_err(|_| Failure::from("unsupported_response"))?;
    let response_account = text(&response["account_id"]).or_else(|| text(&response["accountId"]));
    if let (Some(expected), Some(actual)) = (c.account_id.as_deref(), response_account)
        && expected != actual
    {
        return Err("account_mismatch".into());
    }
    Ok(response)
}
fn window(v: &Value) -> Option<QuotaWindow> {
    let used = v["used_percent"]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0)?;
    let seconds = v["limit_window_seconds"].as_u64().filter(|v| *v > 0)?;
    let reset = v["reset_at"]
        .as_i64()
        .filter(|s| chrono::DateTime::from_timestamp(*s, 0).is_some())?;
    Some(QuotaWindow {
        remaining_percent: Some((100.0 - used).clamp(0.0, 100.0)),
        duration_minutes: Some(seconds.div_ceil(60)),
        resets_at: Some(reset),
    })
}
fn normalize(source: &str, identity: &str, data: &Value) -> Result<Vec<QuotaBucket>, Failure> {
    let captured = chrono::Utc::now().to_rfc3339();
    let mut rows = vec![("codex".to_owned(), "Codex".to_owned(), &data["rate_limit"])];
    if let Some(extra) = data["additional_rate_limits"].as_array() {
        rows.extend(extra.iter().enumerate().map(|(i, e)| {
            (
                text(&e["metered_feature"])
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("extra_{i}")),
                text(&e["limit_name"]).unwrap_or("额外额度").to_owned(),
                &e["rate_limit"],
            )
        }));
    }
    let mut buckets = Vec::new();
    for (id, name, rate) in rows {
        let primary = window(&rate["primary_window"]);
        let secondary = window(&rate["secondary_window"]);
        if primary.is_none() && secondary.is_none() {
            continue;
        }
        buckets.push(QuotaBucket {
            source_id: source.into(),
            identity_key: identity.into(),
            identity_confirmed: true,
            limit_id: id,
            name,
            plan: text(&data["plan_type"]).map(str::to_owned),
            primary,
            secondary,
            captured_at: captured.clone(),
        });
    }
    if buckets.is_empty() {
        return Err("unsupported_response".into());
    }
    Ok(buckets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
        time::Instant,
    };
    fn fixture() -> (tempfile::TempDir, Scope) {
        let dir = tempfile::tempdir().unwrap();
        let scope = Scope {
            source_id: "test".into(),
            home: dir.path().into(),
            wsl: Some(crate::source_config::WslTarget {
                distro: "test".into(),
                user: String::new(),
            }),
        };
        auth(&scope, "account-a", "subject-a");
        (dir, scope)
    }
    fn auth(scope: &Scope, account: &str, subject: &str) {
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({"sub":subject,"exp":chrono::Utc::now().timestamp()+3600}))
                .unwrap(),
        );
        std::fs::write(scope.home.join("auth.json"), serde_json::to_vec(&json!({"auth_mode":"chatgpt","tokens":{"access_token":format!("header.{payload}.signature"),"account_id":account}})).unwrap()).unwrap();
    }
    fn response() -> Value {
        json!({"account_id":"account-a","plan_type":"pro","rate_limit":{"primary_window":{"used_percent":25,"reset_at":1800000000,"limit_window_seconds":18000}},
            "additional_rate_limits":[{"limit_name":"Spark","metered_feature":"spark","rate_limit":{"secondary_window":{"used_percent":40,"reset_at":1800000100,"limit_window_seconds":604800}}}]})
    }
    fn serve(status: &str, headers: &str, body: &str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/usage", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
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
            stream.write_all(response.as_bytes()).unwrap();
            request
        });
        (url, handle)
    }
    #[test]
    #[ignore = "requires explicit access to the current local Codex login and network"]
    fn live_account_readouts() {
        let scope = Scope {
            source_id: "macos".into(),
            home: std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".codex")),
            wsl: None,
        };
        let (_, quota) = checked_fetch(&scope, |_| {}, |c| fetch(&scope, c));
        let quota = quota.unwrap();
        assert!(!quota.buckets.is_empty());
        assert!(quota.allowance.balance.is_some() || quota.allowance.unlimited);
        assert!(quota.allowance.reset_cards.is_some());
        assert_eq!(quota.allowance.expiration_status, "connected");
        println!("{}", serde_json::to_string(&quota.allowance).unwrap());
        let (_, profile) = checked_fetch(&scope, |_| {}, fetch_profile);
        let profile = profile.unwrap();
        assert!(profile.username.is_some() || profile.display_name.is_some());
        assert!(!profile.stats_unavailable);
        assert!(profile.lifetime_tokens.is_some());
        assert!(profile.peak_daily_tokens.is_some());
        assert!(profile.longest_running_turn_sec.is_some());
        assert!(profile.longest_streak_days.is_some() && profile.current_streak_days.is_some());
    }
    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(3)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .into()
    }
    #[test]
    fn credential_reload_detects_external_switch_and_does_not_modify_auth() {
        let (_dir, scope) = fixture();
        let first = load(&scope).unwrap();
        let bytes = std::fs::read(scope.home.join("auth.json")).unwrap();
        assert_eq!(first.stamp.status, "ready");
        assert!(first.stamp == probe(&scope));
        assert_eq!(std::fs::read(scope.home.join("auth.json")).unwrap(), bytes);
        auth(&scope, "account-b", "subject-b");
        let second = probe(&scope);
        assert_ne!(first.stamp.identity, second.identity);
        assert_ne!(first.stamp.revision, second.revision);
        for store in ["keyring", "auto", "ephemeral"] {
            std::fs::write(
                scope.home.join("config.toml"),
                format!("cli_auth_credentials_store = '{store}'"),
            )
            .unwrap();
            assert!(probe(&scope).identity.is_none());
            assert!(!probe(&scope).status.is_empty());
        }
    }
    #[test]
    fn account_changed_while_request_running_discards_even_a_successful_old_response() {
        let (_dir, scope) = fixture();
        let initial = probe(&scope);
        let mut saw_start = false;
        let (current, result) = checked_fetch(
            &scope,
            |stamp| {
                assert!(stamp == &initial);
                saw_start = true;
            },
            |credentials| {
                // External owner publishes another login while the old request is in flight.
                auth(&scope, "account-b", "subject-b");
                Ok(Readout {
                    allowance: Default::default(),
                    allowance_retry_after: 0,
                    buckets: normalize(
                        "test",
                        credentials.stamp.identity.as_deref().unwrap(),
                        &response(),
                    )
                    .unwrap(),
                    proxy_source: "direct".into(),
                })
            },
        );
        assert!(saw_start);
        assert_eq!(result.unwrap_err().code, "credentials_changed");
        assert_ne!(current.identity, initial.identity);
    }
    #[test]
    fn partial_identity_is_not_confirmed_or_merged_across_sources() {
        let (_dir, mut scope) = fixture();
        let mut data: Value =
            serde_json::from_slice(&std::fs::read(scope.home.join("auth.json")).unwrap()).unwrap();
        data["tokens"]["account_id"] = Value::Null;
        std::fs::write(
            scope.home.join("auth.json"),
            serde_json::to_vec(&data).unwrap(),
        )
        .unwrap();
        let a = load(&scope).unwrap();
        assert!(!a.identity_confirmed);
        scope.source_id = "other".into();
        let b = load(&scope).unwrap();
        assert_ne!(a.stamp.identity, b.stamp.identity);
    }
    #[test]
    fn source_dotenv_proxy_priority_no_proxy_and_bad_config() {
        let (p, source) = proxy_config(b"export HTTPS_PROXY='http://127.0.0.1:8080'\nALL_PROXY=socks5://127.0.0.1:9999\nNO_PROXY=localhost,127.0.0.1", false).unwrap();
        assert_eq!(source, "codex_env");
        assert_eq!(p.unwrap().port(), 8080);
        assert!(
            proxy_config(
                b"HTTPS_PROXY=http://localhost:1\nNO_PROXY=.chatgpt.com,chatgpt.com",
                false
            )
            .unwrap()
            .0
            .is_none()
        );
        assert_eq!(
            proxy_config(b"HTTPS_PROXY=bad://host", false)
                .unwrap_err()
                .code,
            "proxy_config_invalid"
        );
        assert!(bypass("*.example.com", "a.example.com"));
        assert!(!bypass("example.com", "badexample.com"));
    }
    #[test]
    fn public_proxy_reads_routing_without_auth_and_uses_the_destination_host() {
        let dir = tempfile::tempdir().unwrap();
        let scope = Scope {
            source_id: "test".into(),
            home: dir.path().into(),
            wsl: None,
        };
        let env = dir.path().join(".env");
        std::fs::write(
            &env,
            "HTTPS_PROXY=http://127.0.0.1:8080\nNO_PROXY=chatgpt.com",
        )
        .unwrap();
        assert_eq!(
            public_proxy(std::slice::from_ref(&scope), "codex-resets.com")
                .unwrap()
                .unwrap()
                .port(),
            8080
        );
        assert!(
            public_proxy(std::slice::from_ref(&scope), "chatgpt.com")
                .unwrap()
                .is_none()
        );
        std::fs::write(
            &env,
            "HTTPS_PROXY=socks5://127.0.0.1:9999\nNO_PROXY=.codex-resets.com",
        )
        .unwrap();
        assert!(
            public_proxy(std::slice::from_ref(&scope), "api.codex-resets.com")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            public_proxy(std::slice::from_ref(&scope), "codex-resets.com")
                .unwrap()
                .unwrap()
                .port(),
            9999
        );
        std::fs::write(&env, "HTTPS_PROXY=bad://host").unwrap();
        assert_eq!(
            public_proxy(std::slice::from_ref(&scope), "codex-resets.com")
                .unwrap_err()
                .code,
            "proxy_config_invalid"
        );
        // Each request reloads edits; no proxy URL or auth file is cached.
        std::fs::write(&env, "HTTPS_PROXY=\n").unwrap();
        assert!(
            public_proxy(std::slice::from_ref(&scope), "codex-resets.com")
                .unwrap()
                .is_none()
        );
        assert!(!dir.path().join("auth.json").exists());
    }
    #[test]
    fn updater_routing_reloads_dotenv_and_matches_both_clients_without_auth() {
        let dir = tempfile::tempdir().unwrap();
        let scope = Scope {
            source_id: "test".into(),
            home: dir.path().into(),
            wsl: None,
        };
        // Invalid account files must not prevent anonymous updates.
        std::fs::write(dir.path().join("auth.json"), "not an auth file").unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "HTTPS_PROXY=http://user:pass@127.0.0.1:8080\nALL_PROXY=http://localhost:9999\nNO_PROXY=api.github.com, .INTERNAL.test, *.example.test\nAPI_KEY=ignored\n").unwrap();
        let scopes = [scope];
        let routing = public_proxy_routing(&scopes).unwrap().unwrap();
        let proxy = routing.ureq_proxy().unwrap();
        assert_eq!(proxy.port(), 8080);
        assert_eq!(proxy.username(), Some("user"));
        assert_eq!(proxy.password(), Some("pass"));
        for (host, direct) in [
            ("api.github.com", true),
            ("github.com", false),
            ("release-assets.githubusercontent.com", false),
            ("api.internal.test", true),
            ("internal.test", false),
            ("a.example.test", true),
            ("example.test", false),
        ] {
            let url = format!("https://{host}/asset");
            assert_eq!(routing.for_url(&url.parse().unwrap()).is_none(), direct);
            assert_eq!(proxy.is_no_proxy(&url.parse().unwrap()), direct);
        }
        for (value, scheme) in [
            ("socks://127.0.0.1:1080", "socks5"),
            ("socks5h://127.0.0.1:1080", "socks5h"),
            ("127.0.0.1:1080", "http"),
        ] {
            std::fs::write(&path, format!("HTTPS_PROXY={value}\nNO_PROXY=*")).unwrap();
            let routing = public_proxy_routing(&scopes).unwrap().unwrap();
            assert_eq!(routing.url.as_ref().unwrap().scheme(), scheme);
            assert!(
                routing
                    .for_url(&"https://github.com/a".parse().unwrap())
                    .is_none()
            );
            assert!(
                routing
                    .ureq_proxy()
                    .unwrap()
                    .is_no_proxy(&"https://github.com/a".parse().unwrap())
            );
        }
        std::fs::write(&path, "HTTPS_PROXY=\nALL_PROXY=http://localhost:9999").unwrap();
        let routing = public_proxy_routing(&scopes).unwrap().unwrap();
        assert!(routing.ureq_proxy().is_none());
        assert!(
            routing
                .for_url(&"https://github.com/a".parse().unwrap())
                .is_none()
        );
        std::fs::write(&path, "HTTPS_PROXY=invalid://private-secret").unwrap();
        assert_eq!(
            public_proxy_routing(&scopes).err().unwrap().code,
            "proxy_config_invalid"
        );
        std::fs::write(&path, vec![b'x'; 256 * 1024 + 1]).unwrap();
        assert_eq!(
            public_proxy_routing(&scopes).err().unwrap().code,
            "proxy_config_unreadable"
        );
    }
    #[test]
    fn direct_request_headers_windows_and_additional_buckets() {
        let (_dir, scope) = fixture();
        let credentials = load(&scope).unwrap();
        let (url, server) = serve("200 OK", "", &response().to_string());
        let result = fetch_with(&agent(), &url, &scope, &credentials).unwrap();
        let request = server.join().unwrap().to_ascii_lowercase();
        assert!(request.contains("authorization: bearer header."));
        assert!(request.contains("chatgpt-account-id: account-a"));
        assert_eq!(result.buckets.len(), 2);
        assert_eq!(
            result.buckets[0]
                .primary
                .as_ref()
                .unwrap()
                .remaining_percent,
            Some(75.0)
        );
        assert_eq!(
            result.buckets[1]
                .secondary
                .as_ref()
                .unwrap()
                .duration_minutes,
            Some(10080)
        );
        assert!(
            !serde_json::to_string(&result.buckets)
                .unwrap()
                .contains("account-a")
        );
        assert!(
            !serde_json::to_string(&result.buckets)
                .unwrap()
                .contains(&credentials.token)
        );
    }
    #[test]
    fn throttles_redirects_and_incompatible_or_wrong_account_are_statuses_only() {
        let (_dir, scope) = fixture();
        let credentials = load(&scope).unwrap();
        for (status, headers, expected) in [
            ("429 Too Many Requests", "Retry-After: 1200\r\n", "http_429"),
            ("401 Unauthorized", "", "reauth_required"),
            (
                "302 Found",
                "Location: http://127.0.0.1:1/should-not-follow\r\n",
                "http_302",
            ),
        ] {
            let (url, server) = serve(status, headers, "");
            let e = fetch_with(&agent(), &url, &scope, &credentials).unwrap_err();
            assert_eq!(e.code, expected);
            if expected == "http_429" {
                assert_eq!(e.retry_after, 1200);
            }
            server.join().unwrap();
        }
        let mut wrong = response();
        wrong["account_id"] = json!("account-b");
        let (url, server) = serve("200 OK", "", &wrong.to_string());
        assert_eq!(
            fetch_with(&agent(), &url, &scope, &credentials)
                .unwrap_err()
                .code,
            "account_mismatch"
        );
        server.join().unwrap();
        assert_eq!(
            normalize("test", "hash", &json!({"unexpected":1}))
                .unwrap_err()
                .code,
            "unsupported_response"
        );
    }
    #[test]
    #[ignore = "explicit live HTTPS benchmark; no CLI is launched"]
    fn measure_live_https_refresh() {
        let report = std::env::var_os("PULSE_BENCH_REPORT").expect("report path");
        let mut scopes = vec![Scope {
            source_id: "windows".into(),
            home: std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex")
                }),
            wsl: None,
        }];
        if let Some(home) = std::env::var_os("PULSE_BENCH_WSL_HOME") {
            scopes.push(Scope {
                source_id: "wsl".into(),
                home: PathBuf::from(home),
                wsl: Some(crate::source_config::WslTarget {
                    distro: std::env::var("PULSE_BENCH_WSL_DISTRO").unwrap(),
                    user: String::new(),
                }),
            });
        }
        let mut samples = Vec::new();
        for scope in scopes {
            let start = Instant::now();
            let (status, proxy) = match load(&scope) {
                Ok(c) => {
                    let status = match fetch(&scope, &c) {
                        Ok(_) => "success".into(),
                        Err(e) => e.code,
                    };
                    assert!(
                        c.stamp == probe(&scope),
                        "credentials changed during sample"
                    );
                    (status, c.proxy_source)
                }
                Err(e) => (e.code, "unavailable".into()),
            };
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{}: {elapsed_ms:.2} ms, {status}, proxy={proxy}",
                scope.source_id
            );
            samples.push(json!({"source":scope.source_id,"status":status,"elapsed_ms":elapsed_ms,"proxy_source":proxy}));
        }
        std::fs::write(
            report,
            serde_json::to_vec_pretty(
                &json!({"timestamp":chrono::Utc::now().to_rfc3339(),"samples":samples}),
            )
            .unwrap(),
        )
        .unwrap();
    }
}
