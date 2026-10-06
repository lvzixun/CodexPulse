//! Bounded public cursor traversal. Validators belong to individual page URLs.
use super::{Cache, FetchError};
use pulse_core::news::{MAX_HISTORY_ROWS, history_items};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const ENDPOINT: &str = "https://codex-resets.com/api/v1/resets";
const MAX_PAGES: usize = 10;
const MAX_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    cursor: Option<String>,
    cache: Cache,
}

pub(super) fn fetch(
    old: &[Page],
    legacy: &Cache,
    cancelled: &dyn Fn() -> bool,
    request: impl FnMut(&str, &Cache, Duration) -> Result<(Cache, i64), FetchError>,
) -> Result<(Cache, Vec<Page>, i64), FetchError> {
    let deadline = Instant::now() + Duration::from_secs(15);
    fetch_until(old, legacy, cancelled, request, deadline)
}
fn fetch_until(
    old: &[Page],
    legacy: &Cache,
    cancelled: &dyn Fn() -> bool,
    mut request: impl FnMut(&str, &Cache, Duration) -> Result<(Cache, i64), FetchError>,
    deadline: Instant,
) -> Result<(Cache, Vec<Page>, i64), FetchError> {
    let empty = Cache::default();
    let mut cursor: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    let mut seen_ids = HashSet::new();
    let mut rows = Vec::<Value>::new();
    let mut pages = Vec::new();
    let mut bytes = 0usize;
    let mut wait = 0i64;
    let mut no_store = false;
    for _ in 0..MAX_PAGES {
        if cancelled() {
            return Err(("cancelled".into(), 0));
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|v| !v.is_zero())
            .ok_or(("history_timeout".into(), 300))?;
        let mut url = url::Url::parse(ENDPOINT).expect("fixed public endpoint");
        url.query_pairs_mut()
            .append_pair("limit", "100")
            .append_pair("order", "desc");
        if let Some(cursor) = &cursor {
            url.query_pairs_mut().append_pair("cursor", cursor);
        }
        let cached = old
            .iter()
            .take(MAX_PAGES)
            .find(|p| p.cursor == cursor)
            .map(|p| &p.cache)
            .unwrap_or(if cursor.is_none() && old.is_empty() {
                legacy
            } else {
                &empty
            });
        let (cache, ttl) = request(url.as_str(), cached, remaining.min(Duration::from_secs(10)))?;
        if cancelled() {
            return Err(("cancelled".into(), 0));
        }
        if Instant::now() >= deadline {
            return Err(("history_timeout".into(), 300));
        }
        let body = cache
            .body
            .as_ref()
            .ok_or(("missing_history_cache".into(), 300))?;
        let parsed = history_items(body).map_err(|e| (e.into(), 300))?;
        let data = body["data"]
            .as_array()
            .ok_or(("invalid_history".into(), 300))?;
        if data.len() > 100 {
            return Err(("history_page_too_large".into(), 300));
        }
        bytes = bytes.saturating_add(
            serde_json::to_vec(body)
                .map_err(|_| ("invalid_history".into(), 300))?
                .len(),
        );
        if bytes > MAX_BYTES {
            return Err(("history_too_large".into(), 300));
        }
        let before = rows.len();
        for (row, parsed) in data.iter().zip(parsed) {
            if seen_ids.insert(parsed.id) {
                rows.push(row.clone());
            }
        }
        if rows.len() > MAX_HISTORY_ROWS {
            return Err(("history_too_large".into(), 300));
        }
        let more = body["pagination"]["has_more"]
            .as_bool()
            .ok_or(("invalid_history_pagination".into(), 300))?;
        let next = if more {
            let next = body["pagination"]["next_cursor"]
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 1024)
                .ok_or(("invalid_history_cursor".into(), 300))?;
            if rows.len() == before || !seen_cursors.insert(next.to_owned()) {
                return Err(("history_cursor_did_not_advance".into(), 300));
            }
            Some(next.to_owned())
        } else {
            None
        };
        wait = wait.max(ttl);
        no_store |= cache.no_store;
        pages.push(Page { cursor, cache });
        if !more {
            return Ok((
                Cache {
                    body: Some(json!({"meta":{"api_version":"v1"},"data":rows,
                    "pagination":{"has_more":false,"next_cursor":null}})),
                    fetched_at: Some(chrono::Utc::now().to_rfc3339()),
                    no_store,
                    // An aggregate must never use one page's validator.
                    etag: None,
                },
                pages,
                wait,
            ));
        }
        cursor = next;
    }
    Err(("history_limit_exceeded".into(), 300))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: usize, at: &str) -> Value {
        json!({"id":format!("synthetic-{id}"),"reset_type":"regular",
            "announced_at":at,"text":"Synthetic public reset","source":{"type":"observed"}})
    }
    fn page(rows: Vec<Value>, next: Option<&str>, etag: &str) -> Cache {
        Cache {
            body: Some(json!({"meta":{"api_version":"v1"},"data":rows,
                "pagination":{"has_more":next.is_some(),"next_cursor":next}})),
            etag: Some(etag.into()),
            ..Cache::default()
        }
    }

    #[test]
    fn crosses_first_page_boundary_deduplicates_and_revalidates_each_url() {
        let first = page(
            (0..100).map(|id| row(id, "2026-01-01T00:00:00Z")).collect(),
            Some("opaque +/&?"),
            "head",
        );
        let second = page(
            vec![
                row(99, "2026-01-01T00:00:00Z"),
                row(100, "2025-01-01T00:00:00Z"),
            ],
            None,
            "tail",
        );
        let mut requests = 0;
        let (aggregate, pages, wait) =
            fetch(&[], &Cache::default(), &|| false, |uri, cached, timeout| {
                let url = url::Url::parse(uri).unwrap();
                assert_eq!(url.host_str(), Some("codex-resets.com"));
                assert!(timeout <= Duration::from_secs(10));
                assert!(cached.etag.is_none());
                requests += 1;
                if requests == 1 {
                    Ok((first.clone(), 300))
                } else {
                    assert!(
                        url.query_pairs()
                            .any(|(k, v)| k == "cursor" && v == "opaque +/&?")
                    );
                    Ok((second.clone(), 900))
                }
            })
            .unwrap();
        assert_eq!(requests, 2);
        assert_eq!(wait, 900);
        assert!(aggregate.etag.is_none());
        let stats = pulse_core::news::reset_stats(&Value::Null, aggregate.body.as_ref().unwrap());
        assert!(stats.history_complete);
        assert_eq!(stats.total, Some(101));
        assert_eq!(stats.longest_wait_days, Some(365.0));
        let mut validators = Vec::new();
        fetch(&pages, &aggregate, &|| false, |_, cached, _| {
            validators.push(cached.etag.clone().unwrap());
            Ok((cached.clone(), 300))
        })
        .unwrap();
        assert_eq!(validators, ["head", "tail"]);
    }

    #[test]
    fn bad_or_repeated_cursors_and_empty_progress_are_errors() {
        for cursor in ["", "repeat"] {
            let mut calls = 0;
            let result = fetch(&[], &Cache::default(), &|| false, |_, _, _| {
                calls += 1;
                Ok((
                    page(
                        vec![row(calls, "2026-01-01T00:00:00Z")],
                        Some(cursor),
                        "page",
                    ),
                    300,
                ))
            });
            assert!(result.is_err());
            assert!(calls <= 2);
        }
        let result = fetch(&[], &Cache::default(), &|| false, |_, _, _| {
            Ok((page(vec![], Some("next"), "page"), 300))
        });
        assert_eq!(result.unwrap_err().0, "history_cursor_did_not_advance");
    }

    #[test]
    fn transport_failure_and_cancellation_do_not_publish_partial_history() {
        let old = vec![Page {
            cursor: None,
            cache: page(vec![row(0, "2026-01-01T00:00:00Z")], None, "old"),
        }];
        let preserved = serde_json::to_value(&old).unwrap();
        let mut calls = 0;
        let result = fetch(&old, &Cache::default(), &|| false, |_, _, _| {
            calls += 1;
            if calls == 1 {
                Ok((
                    page(vec![row(1, "2026-01-02T00:00:00Z")], Some("next"), "new"),
                    300,
                ))
            } else {
                Err(("http_429".into(), 1200))
            }
        });
        assert_eq!(result.unwrap_err(), ("http_429".into(), 1200));
        assert_eq!(serde_json::to_value(&old).unwrap(), preserved);
        let cancelled = std::cell::Cell::new(false);
        let result = fetch(&old, &Cache::default(), &|| cancelled.get(), |_, _, _| {
            cancelled.set(true);
            Ok((
                page(vec![row(1, "2026-01-02T00:00:00Z")], Some("next"), "new"),
                300,
            ))
        });
        assert_eq!(result.unwrap_err().0, "cancelled");
    }

    #[test]
    fn tail_no_store_discards_the_entire_aggregate_and_page_validators() {
        let mut calls = 0;
        let (aggregate, pages, _) = fetch(&[], &Cache::default(), &|| false, |_, _, _| {
            calls += 1;
            let mut cache = page(
                vec![row(calls, "2026-01-01T00:00:00Z")],
                if calls == 1 { Some("next") } else { None },
                "page",
            );
            cache.no_store = calls == 2;
            Ok((cache, 300))
        })
        .unwrap();
        assert!(aggregate.no_store);
        let stored = super::super::Feed {
            history_cache: aggregate,
            history_pages: pages,
            ..Default::default()
        }
        .for_storage();
        assert!(stored.history_cache.body.is_none());
        assert!(stored.history_pages.is_empty());
    }

    #[test]
    fn page_count_and_aggregate_bytes_have_hard_limits() {
        let mut calls = 0;
        let result = fetch(&[], &Cache::default(), &|| false, |_, _, _| {
            calls += 1;
            Ok((
                page(
                    vec![row(calls, "2026-01-01T00:00:00Z")],
                    Some(&format!("cursor-{calls}")),
                    "page",
                ),
                300,
            ))
        });
        assert_eq!(calls, MAX_PAGES);
        assert_eq!(result.unwrap_err().0, "history_limit_exceeded");
        let mut calls = 0;
        let result = fetch(&[], &Cache::default(), &|| false, |_, _, _| {
            calls += 1;
            let mut value = row(calls, "2026-01-01T00:00:00Z");
            value["text"] = "x".repeat(300 * 1024).into();
            Ok((
                page(vec![value], Some(&format!("cursor-{calls}")), "page"),
                300,
            ))
        });
        assert_eq!(calls, 2);
        assert_eq!(result.unwrap_err().0, "history_too_large");
    }

    #[test]
    fn expired_deadline_starts_no_transport() {
        let result = fetch_until(
            &[],
            &Cache::default(),
            &|| false,
            |_, _, _| panic!("expired deadline must not issue a request"),
            Instant::now() - Duration::from_millis(1),
        );
        assert_eq!(result.unwrap_err().0, "history_timeout");
    }
}
