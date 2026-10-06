use chrono::NaiveDate;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Challenge {
    pub start_date: String,
    pub days: u32,
    pub timezone: String,
    pub records: Vec<ChallengeDay>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChallengeDay {
    pub day: u32,
    pub date: String,
    pub entries: Vec<ChallengeEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChallengeEntry {
    pub kind: String,
    pub title: String,
    pub text: String,
    pub source_url: Option<String>,
}
fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("constant challenge selector")
}
fn text(element: ElementRef<'_>, limit: usize) -> String {
    element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

/// The public API does not contain the challenge. Parse its server-rendered page,
/// retaining only bounded article text, never scripts, votes or the full document.
pub fn challenge_page(html: &str) -> Result<Challenge, &'static str> {
    if html.len() > 512 * 1024 {
        return Err("challenge_body_too_large");
    }
    let document = Html::parse_document(html);
    let clock = document
        .select(&selector("[data-challenge-clock]"))
        .next()
        .ok_or("unsupported_challenge_page")?;
    let start = clock
        .value()
        .attr("data-start")
        .ok_or("missing_challenge_start")?;
    let start_date =
        NaiveDate::parse_from_str(start, "%Y-%m-%d").map_err(|_| "invalid_challenge_start")?;
    let days: u32 = clock
        .value()
        .attr("data-days")
        .and_then(|s| s.parse().ok())
        .filter(|d| *d == 28)
        .ok_or("unsupported_challenge_length")?;
    let mut records = Vec::new();
    let rows = selector(".challenge-ledger .challenge-row");
    let articles = selector(".challenge-entry");
    let heading = selector("h3");
    let link = selector("h3 a[href]");
    let paragraphs = selector("p");
    let date = selector(".challenge-row-date time[datetime]");
    for row in document.select(&rows).take(29) {
        let day: u32 = row
            .value()
            .attr("id")
            .and_then(|v| v.strip_prefix("day-"))
            .and_then(|v| v.parse().ok())
            .filter(|d| (1..=days).contains(d))
            .ok_or("invalid_challenge_day")?;
        if records.iter().any(|r: &ChallengeDay| r.day == day) {
            return Err("duplicate_challenge_day");
        }
        let date_value = row
            .select(&date)
            .next()
            .and_then(|v| v.value().attr("datetime"))
            .ok_or("missing_challenge_day_date")?;
        let expected = start_date
            .checked_add_days(chrono::Days::new((day - 1).into()))
            .ok_or("invalid_challenge_day_date")?
            .to_string();
        if date_value != expected {
            return Err("invalid_challenge_day_date");
        }
        let mut entries = Vec::new();
        for article in row.select(&articles).take(4) {
            let kind = if article.value().has_class(
                "challenge-entry--improvement",
                scraper::CaseSensitivity::CaseSensitive,
            ) {
                "improvement"
            } else if article.value().has_class(
                "challenge-entry--reset",
                scraper::CaseSensitivity::CaseSensitive,
            ) {
                "reset"
            } else {
                "update"
            };
            let title = article
                .select(&heading)
                .next()
                .map(|v| text(v, 240))
                .filter(|v| !v.is_empty())
                .ok_or("missing_challenge_entry_title")?;
            let body = article
                .select(&paragraphs)
                .take(4)
                .map(|v| text(v, 1200))
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(2400)
                .collect();
            let source_url = article
                .select(&link)
                .next()
                .and_then(|v| v.value().attr("href"))
                .filter(|v| {
                    v.len() <= 512
                        && (v.starts_with("https://x.com/thsottiaux/status/")
                            || v.starts_with("https://twitter.com/thsottiaux/status/"))
                })
                .map(str::to_owned);
            entries.push(ChallengeEntry {
                kind: kind.into(),
                title,
                text: body,
                source_url,
            });
        }
        records.push(ChallengeDay {
            day,
            date: expected,
            entries,
        });
    }
    if records.len() > 28 {
        return Err("invalid_challenge_records");
    }
    records.sort_by_key(|r| std::cmp::Reverse(r.day));
    Ok(Challenge {
        start_date: start_date.to_string(),
        days,
        timezone: "America/Los_Angeles".into(),
        records,
    })
}
