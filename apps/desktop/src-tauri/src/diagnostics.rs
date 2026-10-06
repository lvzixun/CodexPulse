//! A whitelist projection, not a serialized snapshot followed by string masking.
//! Account/profile/usage/path/error payloads cannot enter this report.
use crate::{backend::Snapshot, window_lifecycle};
use serde::Serialize;

#[derive(Serialize)]
pub struct Window {
    pub generation: u64,
    pub page: &'static str,
    pub glass_supported: bool,
    pub floating_supported: bool,
    pub lifecycle: Option<window_lifecycle::Diagnostics>,
}

#[derive(Serialize)]
pub struct Report {
    format_version: u8,
    redacted: bool,
    app_version: &'static str,
    platform: &'static str,
    architecture: &'static str,
    generated_at: String,
    collecting: bool,
    collector_error: bool,
    timezone_rebuilding: bool,
    timezone_error: bool,
    reference_rebuilding: bool,
    source_count: usize,
    sources: Vec<Source>,
    quota_source_count: usize,
    quota_sources: Vec<QuotaSource>,
    quota: Network,
    news: Network,
    news_status: String,
    challenge_status: String,
    reset_history_complete: bool,
    window: Window,
}
#[derive(Serialize)]
struct Source {
    number: usize,
    status: String,
    files: usize,
    issues: usize,
    last_read: Option<String>,
}
#[derive(Serialize)]
struct QuotaSource {
    number: usize,
    status: String,
    profile_status: String,
    failures: u32,
    last_success: Option<String>,
}
#[derive(Serialize)]
struct Network {
    mode: &'static str,
    interval_seconds: u64,
    request_status: String,
    last_success: Option<String>,
}
fn timestamp(input: Option<&str>) -> Option<String> {
    let value = input.filter(|s| s.len() <= 64)?;
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&chrono::Utc).to_rfc3339())
}
fn status(value: &str) -> String {
    match value {
        "" => "unknown".into(),
        "connected"
        | "ready"
        | "scanning"
        | "no_logs"
        | "read_error"
        | "wsl_stopped"
        | "wsl_unavailable"
        | "refreshing"
        | "queued"
        | "backoff"
        | "idle"
        | "verifying_account"
        | "awaiting_refresh"
        | "credentials_unreadable"
        | "credentials_invalid"
        | "credential_file_too_large"
        | "credential_config_invalid"
        | "credential_store_unsupported"
        | "credentials_ephemeral"
        | "credentials_expired"
        | "not_signed_in"
        | "reauth_required"
        | "auth_mode_unsupported"
        | "custom_backend_unsupported"
        | "account_identity_unavailable"
        | "network_error"
        | "unsupported_response"
        | "account_mismatch"
        | "credentials_changed"
        | "proxy_config_invalid"
        | "body_too_large"
        | "body_read_error"
        | "invalid_json"
        | "invalid_challenge_encoding"
        | "invalid_page_data"
        | "missing_status_cache"
        | "unsupported_api_version"
        | "missing_data"
        | "missing_text"
        | "missing_time"
        | "invalid_time"
        | "invalid_id"
        | "invalid_history"
        | "invalid_watch"
        | "cancelled" => value.into(),
        _ if value.len() == 8
            && value.starts_with("http_")
            && value[5..]
                .parse::<u16>()
                .is_ok_and(|n| (100..600).contains(&n)) =>
        {
            value.into()
        }
        _ => "unknown".into(),
    }
}
fn network(config: &crate::refresh::Config, state: &str, last: Option<&str>) -> Network {
    Network {
        mode: match config.mode.as_str() {
            "manual" => "manual",
            "auto" => "auto",
            _ => "unknown",
        },
        interval_seconds: config.interval_seconds,
        request_status: status(state),
        last_success: timestamp(last),
    }
}
impl Report {
    pub fn from_snapshot(snapshot: &Snapshot, window: Window) -> Self {
        Self {
            format_version: 1,
            redacted: true,
            app_version: env!("CARGO_PKG_VERSION"),
            platform: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            generated_at: chrono::Utc::now().to_rfc3339(),
            collecting: snapshot.collecting,
            collector_error: snapshot.error.is_some(),
            timezone_rebuilding: snapshot.timezone_rebuild.is_some(),
            timezone_error: snapshot.timezone_error.is_some(),
            reference_rebuilding: snapshot.usage.reference.pending,
            source_count: snapshot.sources.len(),
            sources: snapshot
                .sources
                .iter()
                .take(32)
                .enumerate()
                .map(|(i, source)| Source {
                    number: i + 1,
                    status: status(&source.status),
                    files: source.files,
                    issues: source.issues,
                    last_read: timestamp(source.last_read.as_deref()),
                })
                .collect(),
            quota_source_count: snapshot.quota.sources.len(),
            quota_sources: snapshot
                .quota
                .sources
                .values()
                .take(32)
                .enumerate()
                .map(|(i, source)| QuotaSource {
                    number: i + 1,
                    status: status(&source.status),
                    profile_status: status(&source.profile_status),
                    failures: source.failures,
                    last_success: timestamp(source.last_success.as_deref()),
                })
                .collect(),
            quota: network(
                &snapshot.settings.quota_refresh,
                &snapshot.quota.request_status,
                snapshot.quota.last_success.as_deref(),
            ),
            news: network(
                &snapshot.settings.news_refresh,
                &snapshot.news.request_status,
                snapshot.news.last_success.as_deref(),
            ),
            news_status: status(&snapshot.news.status),
            challenge_status: status(&snapshot.news.challenge_status),
            reset_history_complete: snapshot.news.reset_stats.history_complete,
            window,
        }
    }
    pub fn text(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|_| "无法生成诊断信息".into())
    }
}

/// Only the report generated by the backend reaches the system clipboard.
pub fn copy(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let board = objc2_app_kit::NSPasteboard::generalPasteboard();
        board.clearContents();
        let kind = unsafe { objc2_app_kit::NSPasteboardTypeString };
        if board.setString_forType(&objc2_foundation::NSString::from_str(text), kind) {
            return Ok(());
        }
        Err("无法复制诊断信息，请手动选择文本复制".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        Err("请手动选择诊断文本复制".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{QuotaSource, SourceHealth};
    fn window() -> Window {
        Window {
            generation: 2,
            page: "settings",
            glass_supported: true,
            floating_supported: false,
            lifecycle: None,
        }
    }
    #[test]
    fn report_excludes_identity_paths_profile_usage_and_raw_errors_even_without_hide_settings() {
        let secret = "SECRET_ACCOUNT_PATH_TITLE";
        let mut snapshot = Snapshot::default();
        snapshot.settings.windows_home = Some(secret.into());
        snapshot.settings.timezone = secret.into();
        snapshot.error = Some(secret.into());
        snapshot.timezone_error = Some(secret.into());
        snapshot.sources.push(SourceHealth {
            id: secret.into(),
            label: secret.into(),
            path: secret.into(),
            status: "connected".into(),
            last_read: Some(secret.into()),
            files: 17,
            issues: 3,
        });
        snapshot.quota.sources.insert(
            secret.into(),
            QuotaSource {
                home: secret.into(),
                identity: Some(secret.into()),
                proxy_source: secret.into(),
                profile: Some(crate::account_profile::AccountProfile {
                    display_name: Some(secret.into()),
                    username: Some(secret.into()),
                    ..Default::default()
                }),
                status: "http_429".into(),
                profile_status: secret.into(),
                ..Default::default()
            },
        );
        snapshot.news.status = secret.into();
        snapshot.news.last_success = Some(secret.into());
        snapshot.settings.quota_refresh.mode = secret.into();
        let text = Report::from_snapshot(&snapshot, window()).text().unwrap();
        assert!(!text.contains(secret));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["redacted"], true);
        assert_eq!(value["sources"][0]["status"], "connected");
        assert_eq!(value["quota_sources"][0]["status"], "http_429");
        assert_eq!(value["quota_sources"][0]["profile_status"], "unknown");
        assert_eq!(value["sources"][0]["files"], 17);
        assert_eq!(value["collector_error"], true);
        for key in [
            "settings", "usage", "recent", "identity", "profile", "path", "home",
        ] {
            assert!(!text.contains(&format!("\"{key}\":")));
        }
    }
    #[test]
    fn source_payload_is_bounded_and_dates_are_normalized() {
        let snapshot = Snapshot {
            sources: (0..100)
                .map(|_| SourceHealth {
                    id: "ignored".into(),
                    label: "ignored".into(),
                    path: "ignored".into(),
                    status: "connected".into(),
                    last_read: Some("2026-10-06T22:00:00+08:00".into()),
                    files: 1,
                    issues: 0,
                })
                .collect(),
            ..Default::default()
        };
        let value = serde_json::to_value(Report::from_snapshot(&snapshot, window())).unwrap();
        assert_eq!(value["source_count"], 100);
        assert_eq!(value["sources"].as_array().unwrap().len(), 32);
        assert_eq!(
            value["sources"][0]["last_read"],
            "2026-10-06T14:00:00+00:00"
        );
        assert_eq!(status("http_999"), "unknown");
        assert_eq!(status("http_4é"), "unknown");
    }
}
