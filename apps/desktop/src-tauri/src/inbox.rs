use crate::news::{Feed, NewsSnapshot};
use pulse_core::news::{NewsItem, important_reset};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Inbox {
    initialized: bool,
    watermark: String,
    challenge_watermark: String,
    known: BTreeMap<String, String>,
    unread: BTreeMap<String, bool>,
}
pub fn key(item: &NewsItem) -> String {
    format!("{:?}:{}", item.kind, item.id)
}
fn signature(text: String) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
impl Inbox {
    pub fn update(&mut self, feed: &Feed) {
        if feed.status != "connected" {
            return;
        }
        let mut rows = feed
            .items
            .iter()
            .map(|item| {
                (
                    key(item),
                    signature(format!(
                        "{}|{}|{:?}|{:?}",
                        item.text, item.occurred_at, item.reset_type, item.scheduled_for
                    )),
                    important_reset(item),
                    item.occurred_at.clone(),
                    false,
                )
            })
            .collect::<Vec<_>>();
        if let Some(challenge) = &feed.view().challenge {
            for record in &challenge.records {
                for entry in &record.entries {
                    if entry.source_url.is_some()
                        && feed
                            .items
                            .iter()
                            .any(|item| item.source_url == entry.source_url)
                    {
                        continue;
                    }
                    let id = format!(
                        "challenge:{}:{}",
                        record.day,
                        entry.source_url.as_deref().unwrap_or(&entry.title)
                    );
                    rows.push((
                        id,
                        signature(format!("{}|{}", entry.title, entry.text)),
                        entry.kind == "reset",
                        record.date.clone(),
                        true,
                    ));
                }
            }
        }
        // New installations establish a baseline; old history is not presented as new.
        for (id, signature, important, at, challenge) in &rows {
            let changed_kind = !*challenge
                && id.split_once(':').is_some_and(|(_, event_id)| {
                    self.known.keys().any(|old| {
                        old.split_once(':')
                            .is_some_and(|(_, old_id)| old_id == event_id)
                    })
                });
            if self.initialized
                && self.known.get(id) != Some(signature)
                && (self.known.contains_key(id)
                    || changed_kind
                    || *at
                        >= if *challenge {
                            self.challenge_watermark.clone()
                        } else {
                            self.watermark.clone()
                        })
            {
                self.unread.insert(id.clone(), *important);
            }
        }
        self.watermark = rows
            .iter()
            .filter(|r| !r.4)
            .map(|r| &r.3)
            .chain(std::iter::once(&self.watermark))
            .max()
            .cloned()
            .unwrap_or_default();
        self.challenge_watermark = rows
            .iter()
            .filter(|r| r.4)
            .map(|r| &r.3)
            .chain(std::iter::once(&self.challenge_watermark))
            .max()
            .cloned()
            .unwrap_or_default();
        self.known = rows
            .into_iter()
            .map(|(id, signature, _, _, _)| (id, signature))
            .take(256)
            .collect();
        self.unread.retain(|id, _| self.known.contains_key(id));
        self.initialized = true;
    }
    pub fn acknowledge(&mut self, keys: &[String]) {
        for key in keys.iter().take(256) {
            self.unread.remove(key);
        }
    }
    pub fn view(&self, mut view: NewsSnapshot) -> NewsSnapshot {
        view.unread_keys = self.unread.keys().cloned().collect();
        view.important_unread = self.unread.values().filter(|v| **v).count();
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulse_core::news::NewsKind;
    fn item(id: &str, at: &str, kind: NewsKind) -> NewsItem {
        NewsItem {
            id: id.into(),
            kind,
            text: "reset".into(),
            occurred_at: at.into(),
            reset_type: Some("regular".into()),
            scheduled_for: None,
            expires_at: None,
            forecast_window: None,
            probability: None,
            source_type: "x_post".into(),
            author: Some("thsottiaux".into()),
            source_url: None,
        }
    }
    #[test]
    fn baseline_dedupe_restart_and_scoped_acknowledgment() {
        let mut inbox = Inbox::default();
        let mut feed = Feed {
            status: "connected".into(),
            items: vec![item("old", "2026-10-01T00:00:00Z", NewsKind::Announcement)],
            ..Feed::default()
        };
        inbox.update(&feed);
        assert!(inbox.view(feed.view()).unread_keys.is_empty());
        feed.items.insert(
            0,
            item("new", "2026-10-02T00:00:00Z", NewsKind::Announcement),
        );
        inbox.update(&feed);
        let mut inbox: Inbox =
            serde_json::from_str(&serde_json::to_string(&inbox).unwrap()).unwrap();
        inbox.update(&feed);
        assert_eq!(inbox.view(feed.view()).important_unread, 1);
        feed.items
            .insert(0, item("later", "2026-10-03T00:00:00Z", NewsKind::Forecast));
        inbox.update(&feed);
        inbox.acknowledge(&[key(&feed.items[1])]);
        let view = inbox.view(feed.view());
        assert_eq!(view.important_unread, 0);
        assert_eq!(view.unread_keys.len(), 1);
        inbox.update(&feed);
        assert_eq!(inbox.view(feed.view()).unread_keys.len(), 1);
    }
    #[test]
    fn backfill_is_quiet_and_plan_execution_is_important() {
        let mut inbox = Inbox::default();
        let mut feed = Feed {
            status: "connected".into(),
            items: vec![item("plan", "2026-10-02T00:00:00Z", NewsKind::Scheduled)],
            ..Feed::default()
        };
        inbox.update(&feed);
        feed.items
            .push(item("watch", "2026-10-04T00:00:00Z", NewsKind::Forecast));
        inbox.update(&feed);
        inbox.acknowledge(&[key(feed.items.last().unwrap())]);
        feed.items.push(item(
            "backfill",
            "2026-09-01T00:00:00Z",
            NewsKind::Announcement,
        ));
        feed.items[0].kind = NewsKind::Announcement;
        inbox.update(&feed);
        assert_eq!(inbox.view(feed.view()).important_unread, 1);
        feed.items.push(item(
            "unknown",
            "2026-10-05T00:00:00Z",
            NewsKind::Observation,
        ));
        inbox.update(&feed);
        assert_eq!(inbox.view(feed.view()).important_unread, 1);
        assert_eq!(inbox.view(feed.view()).unread_keys.len(), 2);
    }
    #[test]
    fn challenge_and_reset_watermarks_are_independent_and_duplicate_posts_are_quiet() {
        use pulse_core::news::{Challenge, ChallengeDay, ChallengeEntry};
        let mut inbox = Inbox::default();
        let mut feed = Feed {
            status: "connected".into(),
            items: vec![item("old", "2026-10-01T00:00:00Z", NewsKind::Announcement)],
            ..Feed::default()
        };
        let mut challenge = Challenge {
            start_date: "2026-10-05".into(),
            days: 28,
            timezone: "America/Los_Angeles".into(),
            records: vec![ChallengeDay {
                day: 1,
                date: "2026-10-05".into(),
                entries: vec![ChallengeEntry {
                    kind: "improvement".into(),
                    title: "Improvement".into(),
                    text: "Update".into(),
                    source_url: Some("https://x.com/thsottiaux/status/1".into()),
                }],
            }],
        };
        feed.challenge_cache.body = Some(serde_json::to_value(&challenge).unwrap());
        inbox.update(&feed);
        let mut reset = item("new", "2026-10-05T01:00:00Z", NewsKind::Announcement);
        reset.source_url = Some("https://x.com/thsottiaux/status/2".into());
        challenge.records[0].entries.push(ChallengeEntry {
            kind: "reset".into(),
            title: "Reset".into(),
            text: "Reset".into(),
            source_url: reset.source_url.clone(),
        });
        feed.items.push(reset);
        feed.challenge_cache.body = Some(serde_json::to_value(&challenge).unwrap());
        inbox.update(&feed);
        assert_eq!(inbox.view(feed.view()).unread_keys.len(), 1);
        assert_eq!(inbox.view(feed.view()).important_unread, 1);
        challenge.records[0].entries.push(ChallengeEntry {
            kind: "improvement".into(),
            title: "More".into(),
            text: "New".into(),
            source_url: None,
        });
        feed.challenge_cache.body = Some(serde_json::to_value(&challenge).unwrap());
        inbox.update(&feed);
        assert_eq!(inbox.view(feed.view()).unread_keys.len(), 2);
        let saved = serde_json::to_string(&inbox).unwrap();
        assert!(!saved.contains("Improvement"));
    }
}
