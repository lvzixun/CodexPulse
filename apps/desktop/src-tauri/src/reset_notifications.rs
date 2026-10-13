//! New public reset plans are submitted to the OS at most once, independently of the UI.
use crate::{backend::Settings, news::Feed};
use chrono::{DateTime, Utc};
use pulse_core::{
    news::{NewsKind, important_reset},
    storage::{Store, StoreError},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;

const STORAGE_KEY: &str = "reset_plan_notifications";
const NOTIFICATION_ID: i32 = 1;

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    // Keep event identities across restarts and temporary removal from the feed.
    // Text edits and changed plan times must not re-notify the same announcement.
    handled: BTreeSet<String>,
}

pub struct Notice {
    at: Option<DateTime<Utc>>,
    banked: bool,
}

impl Notifications {
    pub fn load(store: &Store) -> Result<Self, StoreError> {
        Ok(store.setting(STORAGE_KEY)?.unwrap_or_default())
    }

    pub fn discover(
        &mut self,
        store: &Store,
        previous: &Feed,
        feed: &Feed,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<Option<Notice>, StoreError> {
        // A cached startup projection, failed fetch, or challenge-only refresh is quiet.
        if feed.status != "connected"
            || feed.status_cache.fetched_at.is_none()
            || feed.status_cache.fetched_at == previous.status_cache.fetched_at
        {
            return Ok(None);
        }
        let Some(item) = feed.scheduled_reset.as_ref() else {
            return Ok(None);
        };
        if item.kind != NewsKind::Scheduled || !important_reset(item) || item.id.is_empty() {
            return Ok(None);
        }
        let Ok(announced) = DateTime::parse_from_rfc3339(&item.occurred_at) else {
            return Ok(None);
        };
        if announced > now {
            return Ok(None);
        }
        let at = match item.scheduled_for.as_deref().filter(|v| !v.is_empty()) {
            Some(value) => match DateTime::parse_from_rfc3339(value) {
                Ok(at) if at > now => Some(at.with_timezone(&Utc)),
                _ => return Ok(None),
            },
            None => None,
        };
        if feed.latest_reset.as_ref().is_some_and(|latest| {
            latest.kind == NewsKind::Announcement
                && important_reset(latest)
                && latest.reset_type == item.reset_type
                && DateTime::parse_from_rfc3339(&latest.occurred_at)
                    .ok()
                    .is_some_and(|executed| {
                        executed <= now && executed >= at.unwrap_or(announced.with_timezone(&Utc))
                    })
        }) {
            return Ok(None);
        }
        let key = format!("{:x}", Sha256::digest(item.id.as_bytes()));
        if self.handled.contains(&key) {
            return Ok(None);
        }
        let mut next = self.clone();
        next.handled.insert(key);
        // Claim durably before submitting: a crash/restart must not duplicate a toast.
        // Discoveries while disabled stay quiet if the switch is enabled later.
        store.set_setting(STORAGE_KEY, &next)?;
        *self = next;
        Ok(enabled.then_some(Notice {
            at,
            banked: item.reset_type.as_deref() == Some("banked"),
        }))
    }
}

impl Notice {
    fn content(&self, language: &str, timezone: &str) -> (&'static str, String) {
        let tz = timezone.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
        let time = self
            .at
            .map(|at| format!("{} ({tz})", at.with_timezone(&tz).format("%m/%d %H:%M")));
        if language == "zh" {
            (
                "Codex 重置计划已公布",
                format!(
                    "{}：{}。以实际执行公告为准。",
                    if self.banked {
                        "备用额度重置"
                    } else {
                        "常规额度重置"
                    },
                    time.as_deref().unwrap_or("时间待定"),
                ),
            )
        } else {
            (
                "Codex reset plan announced",
                format!(
                    "{}: {}. Subject to the actual reset announcement.",
                    if self.banked {
                        "Banked reset"
                    } else {
                        "Regular reset"
                    },
                    time.as_deref().unwrap_or("Time to be announced"),
                ),
            )
        }
    }

    pub fn show(self, app: &tauri::AppHandle, settings: &Settings) {
        let (title, body) = self.content(
            crate::language::effective(&settings.language),
            &settings.timezone,
        );
        if let Err(error) = app
            .notification()
            .builder()
            .id(NOTIFICATION_ID)
            .title(title)
            .body(body)
            .show()
        {
            eprintln!("Unable to submit reset notification: {error}");
        }
    }
}

pub fn setup(app: &tauri::AppHandle) -> tauri_plugin_notification::Result<()> {
    let handle = app.clone();
    app.notification().on_action(move |action| {
        if action.action_id() == "tap"
            && action
                .notification()
                .is_some_and(|n| n.id() == NOTIFICATION_ID)
        {
            if let Some(host) = handle.try_state::<crate::platform::WindowHost>()
                && let Ok(mut view) = host.view.lock()
            {
                view.news_challenge = false;
                view.scroll.insert("news".into(), 0.0);
            }
            crate::platform::show_details(&handle, Some("news"));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulse_core::news::NewsItem;

    fn now() -> DateTime<Utc> {
        "2026-10-13T08:00:00Z".parse().unwrap()
    }

    fn feed() -> Feed {
        Feed {
            status: "connected".into(),
            status_cache: crate::news::Cache {
                fetched_at: Some(now().to_rfc3339()),
                ..Default::default()
            },
            scheduled_reset: Some(NewsItem {
                id: "plan-1".into(),
                kind: NewsKind::Scheduled,
                text: "A reset is planned".into(),
                occurred_at: "2026-10-13T07:00:00Z".into(),
                reset_type: Some("regular".into()),
                scheduled_for: Some("2026-10-13T10:00:00Z".into()),
                expires_at: None,
                forecast_window: None,
                probability: None,
                source_type: "x_post".into(),
                author: Some("thsottiaux".into()),
                source_url: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn new_plan_notifies_immediately_and_persists_across_database_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("pulse.sqlite");
        let feed = feed();
        {
            let store = Store::open(&database).unwrap();
            let mut notifications = Notifications::load(&store).unwrap();
            assert!(
                notifications
                    .discover(&store, &Feed::default(), &feed, true, now())
                    .unwrap()
                    .is_some()
            );
            // Even a repeated response presented as fresh cannot duplicate the notification.
            assert!(
                notifications
                    .discover(&store, &Feed::default(), &feed, true, now())
                    .unwrap()
                    .is_none()
            );
        }
        let store = Store::open(&database).unwrap();
        let mut notifications = Notifications::load(&store).unwrap();
        assert!(
            notifications
                .discover(&store, &Feed::default(), &feed, true, now())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn same_plan_edits_removal_reappearance_and_switch_changes_stay_quiet() {
        let store = Store::in_memory().unwrap();
        let mut notifications = Notifications::default();
        let mut feed = feed();
        assert!(
            notifications
                .discover(&store, &Feed::default(), &feed, false, now())
                .unwrap()
                .is_none()
        );
        feed.scheduled_reset.as_mut().unwrap().text = "Edited".into();
        feed.scheduled_reset.as_mut().unwrap().scheduled_for = Some("2026-10-13T12:00:00Z".into());
        assert!(
            notifications
                .discover(&store, &Feed::default(), &Feed::default(), true, now())
                .unwrap()
                .is_none()
        );
        assert!(
            notifications
                .discover(&store, &Feed::default(), &feed, true, now())
                .unwrap()
                .is_none()
        );
        feed.scheduled_reset.as_mut().unwrap().id = "plan-2".into();
        assert!(
            notifications
                .discover(&store, &Feed::default(), &feed, true, now())
                .unwrap()
                .is_some()
        );
        let mut removed = feed.clone();
        removed.status_cache.fetched_at = Some("2026-10-13T08:05:00Z".into());
        removed.scheduled_reset = None;
        assert!(
            notifications
                .discover(&store, &feed, &removed, true, now())
                .unwrap()
                .is_none()
        );
        feed.status_cache.fetched_at = Some("2026-10-13T08:10:00Z".into());
        assert!(
            notifications
                .discover(&store, &removed, &feed, true, now())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cached_views_errors_and_challenge_only_refreshes_do_not_notify() {
        let store = Store::in_memory().unwrap();
        let mut notifications = Notifications::default();
        let mut current = feed();
        assert!(
            notifications
                .discover(&store, &current, &current, true, now())
                .unwrap()
                .is_none()
        );
        current.challenge_cache.fetched_at = Some("2026-10-13T08:05:00Z".into());
        assert!(
            notifications
                .discover(&store, &feed(), &current, true, now())
                .unwrap()
                .is_none()
        );
        current.status = "network_error".into();
        assert!(
            notifications
                .discover(&store, &Feed::default(), &current, true, now())
                .unwrap()
                .is_none()
        );
        current.status = "connected".into();
        current.status_cache.fetched_at = None;
        assert!(
            notifications
                .discover(&store, &Feed::default(), &current, true, now())
                .unwrap()
                .is_none()
        );
        assert!(notifications.handled.is_empty());
    }

    #[test]
    fn forecasts_untrusted_sources_and_unknown_reset_types_do_not_notify() {
        let store = Store::in_memory().unwrap();
        let mut notifications = Notifications::default();
        for kind in [
            NewsKind::Forecast,
            NewsKind::Observation,
            NewsKind::Announcement,
        ] {
            let mut current = feed();
            current.scheduled_reset.as_mut().unwrap().kind = kind;
            assert!(
                notifications
                    .discover(&store, &Feed::default(), &current, true, now())
                    .unwrap()
                    .is_none()
            );
        }
        for (source, author, reset_type) in [
            ("community", "thsottiaux", "regular"),
            ("x_post", "someone_else", "regular"),
            ("x_post", "thsottiaux", "unknown"),
        ] {
            let mut current = feed();
            let item = current.scheduled_reset.as_mut().unwrap();
            item.source_type = source.into();
            item.author = Some(author.into());
            item.reset_type = Some(reset_type.into());
            assert!(
                notifications
                    .discover(&store, &Feed::default(), &current, true, now())
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn past_or_invalid_times_and_executed_pending_plans_do_not_notify() {
        let store = Store::in_memory().unwrap();
        let mut notifications = Notifications::default();
        for at in ["2026-10-13T07:30:00Z", "2026-10-13T08:00:00Z", "invalid"] {
            let mut current = feed();
            current.scheduled_reset.as_mut().unwrap().scheduled_for = Some(at.into());
            assert!(
                notifications
                    .discover(&store, &Feed::default(), &current, true, now())
                    .unwrap()
                    .is_none()
            );
        }
        let mut current = feed();
        current.scheduled_reset.as_mut().unwrap().scheduled_for = None;
        let mut executed = current.scheduled_reset.clone().unwrap();
        executed.kind = NewsKind::Announcement;
        executed.occurred_at = "2026-10-13T07:30:00Z".into();
        current.latest_reset = Some(executed);
        assert!(
            notifications
                .discover(&store, &Feed::default(), &current, true, now())
                .unwrap()
                .is_none()
        );
        // A different type of reset cannot confirm execution of this plan.
        current.latest_reset.as_mut().unwrap().reset_type = Some("banked".into());
        assert!(
            notifications
                .discover(&store, &Feed::default(), &current, true, now())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn pending_time_is_once_and_content_uses_language_and_timezone() {
        let store = Store::in_memory().unwrap();
        let mut notifications = Notifications::default();
        let mut current = feed();
        current.scheduled_reset.as_mut().unwrap().scheduled_for = None;
        let notice = notifications
            .discover(&store, &Feed::default(), &current, true, now())
            .unwrap()
            .unwrap();
        assert!(notice.content("zh", "Asia/Shanghai").1.contains("时间待定"));
        current.scheduled_reset.as_mut().unwrap().scheduled_for =
            Some("2026-10-13T10:00:00Z".into());
        assert!(
            notifications
                .discover(&store, &Feed::default(), &current, true, now())
                .unwrap()
                .is_none()
        );
        current.scheduled_reset.as_mut().unwrap().id = "new-banked-plan".into();
        current.scheduled_reset.as_mut().unwrap().reset_type = Some("banked".into());
        let notice = notifications
            .discover(&store, &Feed::default(), &current, true, now())
            .unwrap()
            .unwrap();
        assert_eq!(
            notice.content("zh", "Asia/Shanghai").1,
            "备用额度重置：10/13 18:00 (Asia/Shanghai)。以实际执行公告为准。"
        );
        assert_eq!(
            notice.content("en", "America/Los_Angeles").1,
            "Banked reset: 10/13 03:00 (America/Los_Angeles). Subject to the actual reset announcement."
        );
    }
}
