//! Native menu-bar projection of the Windows compact window's activity rules.
//! Uses the collector snapshot only; no network requests or session titles.
use crate::backend::Snapshot;
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub title: String,
    pub tooltip: String,
}

fn age(timestamp: &str, now: DateTime<Utc>) -> Option<i64> {
    let time = DateTime::parse_from_rfc3339(timestamp).ok()?;
    let age = now.signed_duration_since(time).num_milliseconds();
    (age >= 0).then_some(age)
}

fn model_text(value: &str, limit: usize) -> String {
    let clean = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean: String = clean.chars().filter(|c| !c.is_control()).collect();
    let mut chars = clean.chars();
    let mut text: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        text.push('…');
    }
    if text.is_empty() { "—".into() } else { text }
}

pub fn render(snapshot: &Snapshot, now: DateTime<Utc>, language: &str) -> Status {
    let zh = language == "zh";
    let connected = |id: &str| {
        snapshot
            .sources
            .iter()
            .any(|s| s.id == id && s.status == "connected")
    };
    let running: Vec<_> = snapshot
        .recent
        .iter()
        .filter(|s| {
            s.meta.status == "active"
                && age(&s.meta.last_activity, now).is_some_and(|age| age <= 300_000)
                && s.sources.iter().any(|source| connected(source))
        })
        .collect();
    let unknown = snapshot.recent.iter().any(|s| s.meta.status == "active")
        || !snapshot.sources.iter().any(|s| s.status == "connected");
    let state = if !running.is_empty() {
        if zh { "忙" } else { "Busy" }
    } else if unknown {
        "?"
    } else if zh {
        "闲"
    } else {
        "Idle"
    };
    let count = if running.len() > 1 {
        running.len().to_string()
    } else {
        String::new()
    };
    let working = running.first();
    let model = working
        .and_then(|s| s.meta.current_model.as_deref())
        .unwrap_or("—");
    let rate = working
        .and_then(|s| s.meta.output_rate.as_ref())
        .filter(|rate| {
            rate.elapsed_ms > 0
                && (rate.completed || age(&rate.measured_at, now).is_some_and(|age| age <= 60_000))
        })
        .map(|rate| {
            format!(
                "{:.1}",
                rate.output_tokens as f64 * 1000.0 / rate.elapsed_ms as f64
            )
        })
        .unwrap_or_else(|| "—".into());
    let important = snapshot.news.important_unread;
    let unread = snapshot.news.unread_keys.len();
    let issue = snapshot.error.is_some()
        || snapshot
            .quota
            .sources
            .values()
            // Match RefreshSettings' failure classification. Local credential
            // recovery and a hidden/manual panel can legitimately wait without
            // an HTTP failure; absence of a completed readout is not an error.
            .any(|q| {
                !matches!(
                    q.status.as_str(),
                    "" | "connected" | "ready" | "verifying_account" | "awaiting_refresh"
                )
            });
    let mark = if important > 0 {
        " ↻"
    } else if unread > 0 {
        " •"
    } else {
        ""
    };
    let title = format!(
        "{state}{count} · {} · {rate} t/s{mark}{}",
        model_text(model.strip_prefix("gpt-").unwrap_or(model), 12),
        if issue { " !" } else { "" }
    );
    let activity = if !running.is_empty() {
        if zh {
            format!("运行中：{} 个会话", running.len())
        } else {
            format!("Running: {} session(s)", running.len())
        }
    } else if unknown {
        if zh {
            "状态未知：活动记录过期或来源未连接".into()
        } else {
            "Unknown: stale activity or disconnected source".into()
        }
    } else if zh {
        "空闲".into()
    } else {
        "Idle".into()
    };
    let messages = if important > 0 {
        if zh {
            format!("↻ 新重置消息：{important} 条（公告/计划；额度以账户信息为准）")
        } else {
            format!("↻ New reset messages: {important} (announcements/plans; check account limits)")
        }
    } else if unread > 0 {
        if zh {
            format!("新动态：{unread} 条")
        } else {
            format!("New posts: {unread}")
        }
    } else if zh {
        "无新重置消息".into()
    } else {
        "No new reset messages".into()
    };
    let tooltip = if zh {
        format!(
            "CodexPulse\n{activity}\n模型：{}\n输出速度：{rate} tok/s（当前轮次平均，含等待及工具时间）\n{messages}{}",
            model_text(model, 80),
            if issue {
                "\n! 数据来源异常，请打开面板查看"
            } else {
                ""
            }
        )
    } else {
        format!(
            "CodexPulse\n{activity}\nModel: {}\nOutput: {rate} tok/s (turn average, including waits and tools)\n{messages}{}",
            model_text(model, 80),
            if issue {
                "\n! Data source issue; open the panel for details"
            } else {
                ""
            }
        )
    };
    Status { title, tooltip }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::SourceHealth;
    use pulse_core::{
        domain::{OutputRate, SessionMeta},
        storage::RecentSession,
    };
    fn fixture() -> (Snapshot, DateTime<Utc>) {
        let now = "2026-10-07T10:00:00Z".parse().unwrap();
        let snapshot = Snapshot {
            sources: vec![SourceHealth {
                id: "macos".into(),
                label: "".into(),
                path: "".into(),
                status: "connected".into(),
                last_read: None,
                files: 0,
                issues: 0,
            }],
            recent: vec![RecentSession {
                meta: SessionMeta {
                    status: "active".into(),
                    last_activity: "2026-10-07T09:59:00Z".into(),
                    current_model: Some("gpt-6.1-sol".into()),
                    output_rate: Some(OutputRate {
                        output_tokens: 384,
                        elapsed_ms: 10_000,
                        measured_at: "2026-10-07T09:59:45Z".into(),
                        completed: false,
                        service_tier: None,
                    }),
                    title: Some("private session title".into()),
                    ..Default::default()
                },
                sources: vec!["macos".into()],
                models: vec![],
                total: 0,
                events: 0,
                unknown_totals: 0,
                cost_nanousd: 0,
                unpriced_tokens: 0,
                reference: Default::default(),
            }],
            ..Default::default()
        };
        (snapshot, now)
    }
    #[test]
    fn running_model_speed_and_parallel_count_match_compact_window() {
        let (mut s, now) = fixture();
        assert_eq!(render(&s, now, "zh").title, "忙 · 6.1-sol · 38.4 t/s");
        s.recent.push(s.recent[0].clone());
        let status = render(&s, now, "en");
        assert_eq!(status.title, "Busy2 · 6.1-sol · 38.4 t/s");
        assert!(!status.tooltip.contains("private session title"));
    }
    #[test]
    fn disconnected_stale_future_and_completed_activity_are_not_running() {
        let (mut s, now) = fixture();
        s.sources[0].status = "offline".into();
        assert_eq!(render(&s, now, "en").title, "? · — · — t/s");
        s.sources[0].status = "connected".into();
        for time in ["2026-10-07T09:54:59Z", "2026-10-07T10:01:00Z", "bad"] {
            s.recent[0].meta.last_activity = time.into();
            assert_eq!(render(&s, now, "zh").title, "? · — · — t/s");
        }
        s.recent[0].meta.status = "completed".into();
        assert_eq!(render(&s, now, "en").title, "Idle · — · — t/s");
    }
    #[test]
    fn live_speed_expires_without_a_new_snapshot_but_completed_rate_remains() {
        let (mut s, now) = fixture();
        let later = now + chrono::Duration::seconds(61);
        assert_eq!(render(&s, later, "zh").title, "忙 · 6.1-sol · — t/s");
        s.recent[0].meta.output_rate.as_mut().unwrap().completed = true;
        assert_eq!(render(&s, later, "zh").title, "忙 · 6.1-sol · 38.4 t/s");
        s.recent[0].meta.output_rate.as_mut().unwrap().elapsed_ms = 0;
        assert!(render(&s, later, "zh").title.contains("— t/s"));
    }
    #[test]
    fn reset_badge_is_distinct_from_ordinary_posts_and_errors_and_clears_on_read() {
        let (mut s, now) = fixture();
        s.news.unread_keys.push("new".into());
        assert!(render(&s, now, "zh").title.ends_with(" •"));
        s.news.important_unread = 1;
        assert!(render(&s, now, "zh").title.ends_with(" ↻"));
        assert!(render(&s, now, "zh").tooltip.contains("新重置消息：1"));
        s.error = Some("private error details".into());
        let status = render(&s, now, "en");
        assert!(status.title.ends_with(" ↻ !"));
        assert!(!status.tooltip.contains("private error details"));
        s.news.important_unread = 0;
        s.news.unread_keys.clear();
        assert!(!render(&s, now, "zh").title.contains('↻'));
    }
    #[test]
    fn normal_account_waiting_does_not_add_an_error_badge() {
        let (mut s, now) = fixture();
        s.news.important_unread = 1;
        for state in [
            "",
            "awaiting_refresh",
            "verifying_account",
            "ready",
            "connected",
        ] {
            s.quota.sources.insert(
                "macos".into(),
                crate::backend::QuotaSource {
                    status: state.into(),
                    failures: 0,
                    ..Default::default()
                },
            );
            for language in ["zh", "en"] {
                let status = render(&s, now, language);
                assert!(status.title.ends_with(" ↻"), "{state}: {}", status.title);
                assert!(!status.tooltip.contains("\n!"), "{state}");
                assert!(status.title.contains("6.1-sol · 38.4 t/s"));
            }
        }
    }
    #[test]
    fn real_account_errors_remain_visible_and_clear_on_credential_recovery() {
        let (mut s, now) = fixture();
        for state in [
            "credentials_expired",
            "reauth_required",
            "credentials_unreadable",
            "network_error",
            "forbidden",
            "rate_limited",
            "invalid_response",
        ] {
            // Local authentication errors can have zero HTTP failure attempts.
            s.quota.sources.insert(
                "macos".into(),
                crate::backend::QuotaSource {
                    status: state.into(),
                    failures: 0,
                    ..Default::default()
                },
            );
            assert!(render(&s, now, "zh").title.ends_with(" !"), "{state}");
        }
        let source = s.quota.sources.get_mut("macos").unwrap();
        source.status = "awaiting_refresh".into();
        source.failures = 0;
        assert!(!render(&s, now, "zh").title.ends_with(" !"));
        s.error = Some("ledger query failed".into());
        assert!(render(&s, now, "zh").title.ends_with(" !"));
    }
    #[test]
    fn long_models_are_bounded_without_invalid_unicode_or_control_characters() {
        assert_eq!(model_text("  a\n\tb  ", 12), "a b");
        assert_eq!(
            model_text("模型名称特别长的情况，需要省略显示", 12),
            "模型名称特别长的情况，需…"
        );
        assert_eq!(model_text("", 12), "—");
    }
}
