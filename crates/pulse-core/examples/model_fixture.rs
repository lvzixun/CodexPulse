//! Isolated synthetic ledger for native model paging validation. Refuses existing files.
use chrono::{TimeZone, Utc};
use pulse_core::{
    domain::{SessionMeta, TokenCounts, UsageFact},
    ledger::ParserState,
    storage::{FileCursor, Store},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(
            "usage: model_fixture <new-database-path> <model-count> <IANA-timezone>".into(),
        );
    }
    let path = std::path::PathBuf::from(&args[0]);
    if path.exists() {
        return Err("fixture destination already exists".into());
    }
    let count = args[1].parse::<usize>()?;
    if !(21..=1000).contains(&count) {
        return Err("fixture model count must be 21..1000".into());
    }
    let timezone = args[2].parse::<chrono_tz::Tz>()?;
    let mut store = Store::open(path)?;
    let day = Utc::now().with_timezone(&timezone).date_naive();
    let timestamp = timezone
        .from_local_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
        .earliest()
        .ok_or("invalid fixture date")?
        .with_timezone(&Utc)
        .to_rfc3339();
    let session = SessionMeta {
        id: "synthetic-model-session".into(),
        title: Some("模型分页合成验证（非真实用量）".into()),
        status: "completed".into(),
        last_activity: timestamp.clone(),
        ..Default::default()
    };
    let facts = (0..count)
        .map(|n| UsageFact {
            id: format!("synthetic-model-fact-{n}"),
            session_id: session.id.clone(),
            turn_id: None,
            model: format!("validation-model-{n:03}"),
            provider: "synthetic".into(),
            timestamp: timestamp.clone(),
            tokens: if n + 1 == count {
                TokenCounts::default()
            } else {
                TokenCounts {
                    input: Some(10),
                    cached: Some(3),
                    output: Some(2),
                    total: Some(12),
                    ..Default::default()
                }
            },
            service_tier: None,
            request_input: None,
            quality: "synthetic".into(),
            price_version: None,
            cost_nanousd: None,
        })
        .collect::<Vec<_>>();
    let cursor = FileCursor {
        source_id: "synthetic".into(),
        file_key: "synthetic".into(),
        offset: count as u64,
        file_identity: "synthetic".into(),
        modified_ns: "1".into(),
        parser_version: pulse_core::PARSER_VERSION,
        state: ParserState::default(),
    };
    store.commit_batch(&cursor, &[session], &facts, &[], timezone)?;
    store.set_setting("app",&serde_json::json!({"timezone":timezone.name(),"windows_enabled":false,"wsl_enabled":false,
        "wsl_auto_detect":false,"quota_refresh":{"mode":"manual","interval_seconds":300},"news_refresh":{"mode":"manual","interval_seconds":300},
        "theme":"dark","accent":"blue","glass":true,"floating":true,"always_on_top":false}))?;
    println!(
        "Created {count} synthetic model records; all sources disabled and both network groups manual."
    );
    Ok(())
}
