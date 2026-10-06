//! Public reset signals are kept separate from account-specific quota recovery.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

mod challenge;
pub use challenge::{Challenge, ChallengeDay, ChallengeEntry, challenge_page};
mod translations;
pub use translations::translations_page;

pub fn important_reset(item: &NewsItem) -> bool {
    matches!(item.kind, NewsKind::Announcement | NewsKind::Scheduled)
        && item.source_type == "x_post"
        && item.author.as_deref() == Some("thsottiaux")
        && matches!(item.reset_type.as_deref(), Some("regular" | "banked"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NewsKind {
    Announcement,
    Scheduled,
    Forecast,
    Observation,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewsItem {
    pub id: String,
    pub kind: NewsKind,
    pub text: String,
    pub occurred_at: String,
    pub reset_type: Option<String>,
    pub scheduled_for: Option<String>,
    pub expires_at: Option<String>,
    pub forecast_window: Option<String>,
    pub probability: Option<u64>,
    pub source_type: String,
    pub author: Option<String>,
    pub source_url: Option<String>,
}
pub fn status_items(response: &Value) -> Result<Vec<NewsItem>, &'static str> {
    check(response)?;
    let data = &response["data"];
    let mut items = Vec::new();
    if data["latest_reset"].is_object() {
        items.push(reset(&data["latest_reset"], None)?);
    }
    if data["scheduled_reset"].is_object()
        && !matches!(
            data["scheduled_reset"]["status"].as_str(),
            Some("cancelled" | "canceled" | "completed")
        )
    {
        items.push(reset(&data["scheduled_reset"], Some(NewsKind::Scheduled))?);
    }
    let watch = &data["active_watch"];
    if watch.is_object() {
        let at = time(watch, "observed_at")?;
        let source = &watch["source"];
        let id = format!(
            "watch:{:x}",
            Sha256::digest(
                serde_json::to_vec(&(at.clone(), &source["url"])).map_err(|_| "invalid_watch")?
            )
        );
        items.push(NewsItem {
            id,
            kind: NewsKind::Forecast,
            text: bounded_text(watch)?,
            occurred_at: at,
            reset_type: None,
            scheduled_for: None,
            expires_at: Some(time(watch, "expires_at")?),
            forecast_window: watch["forecast_window"].as_str().map(str::to_owned),
            probability: watch["reset_chance_percent"].as_u64().filter(|v| *v <= 100),
            source_type: source["type"].as_str().unwrap_or("unknown").into(),
            author: source["author"].as_str().map(str::to_owned),
            source_url: source["url"].as_str().map(str::to_owned),
        });
    }
    Ok(items)
}
pub fn history_items(response: &Value) -> Result<Vec<NewsItem>, &'static str> {
    check(response)?;
    response["data"]
        .as_array()
        .ok_or("invalid_history")?
        .iter()
        .take(100)
        .map(|row| reset(row, None))
        .collect()
}
pub fn merge(status: Vec<NewsItem>, history: Vec<NewsItem>) -> Vec<NewsItem> {
    let mut items = status;
    items.extend(history);
    let mut keys = std::collections::HashSet::new();
    items.retain(|item| keys.insert((item.id.clone(), format!("{:?}", item.kind))));
    items.sort_by(|a, b| {
        b.occurred_at
            .cmp(&a.occurred_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    items.truncate(100);
    items
}
fn check(response: &Value) -> Result<(), &'static str> {
    if response["meta"]["api_version"] != "v1" {
        return Err("unsupported_api_version");
    }
    if response.get("data").is_none() {
        return Err("missing_data");
    }
    Ok(())
}
fn bounded_text(value: &Value) -> Result<String, &'static str> {
    Ok(value["text"]
        .as_str()
        .ok_or("missing_text")?
        .chars()
        .take(4000)
        .collect())
}
fn time(value: &Value, key: &str) -> Result<String, &'static str> {
    let time = value[key].as_str().ok_or("missing_time")?;
    Ok(chrono::DateTime::parse_from_rfc3339(time)
        .map_err(|_| "invalid_time")?
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
fn reset(value: &Value, kind: Option<NewsKind>) -> Result<NewsItem, &'static str> {
    let source = &value["source"];
    let kind = kind.unwrap_or_else(|| {
        if source["type"] == "x_post" {
            NewsKind::Announcement
        } else {
            NewsKind::Observation
        }
    });
    let id = value["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .ok_or("invalid_id")?
        .into();
    Ok(NewsItem {
        id,
        kind,
        text: bounded_text(value)?,
        occurred_at: time(value, "announced_at")?,
        reset_type: value["reset_type"].as_str().map(str::to_owned),
        scheduled_for: if value["scheduled_for"].is_string() {
            Some(time(value, "scheduled_for")?)
        } else {
            None
        },
        expires_at: None,
        forecast_window: None,
        probability: None,
        source_type: source["type"].as_str().unwrap_or("unknown").into(),
        author: source["author"].as_str().map(str::to_owned),
        source_url: source["url"].as_str().map(str::to_owned),
    })
}
