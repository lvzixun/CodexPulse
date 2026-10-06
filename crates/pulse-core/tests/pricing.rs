use pulse_core::{
    domain::{TokenCounts, UsageFact},
    pricing::PriceBook,
};

fn fact(input: u64) -> UsageFact {
    UsageFact {
        id: "request".into(),
        session_id: "session".into(),
        turn_id: Some("turn".into()),
        model: "gpt-6.1-sol".into(),
        provider: "openai".into(),
        timestamp: "2026-10-06T12:00:00Z".into(),
        tokens: TokenCounts {
            input: Some(input),
            cached: Some(0),
            cache_write: Some(0),
            output: Some(100),
            total: Some(input + 100),
            ..Default::default()
        },
        service_tier: Some("default".into()),
        request_input: Some(input),
        quality: "complete".into(),
        price_version: None,
        cost_nanousd: None,
    }
}

#[test]
fn full_request_at_long_context_boundary_uses_corresponding_price() {
    let book = PriceBook::bundled().unwrap();
    assert_eq!(book.price(&fact(272_000)).unwrap().1, 545_000_000);
    assert_eq!(book.price(&fact(272_001)).unwrap().1, 1_089_504_000);
    let mut aggregate = fact(272_000);
    aggregate.request_input = None;
    assert!(book.price(&aggregate).is_none());
    aggregate.request_input = Some(272_001);
    assert!(book.price(&aggregate).is_none());
}

#[test]
fn cache_writes_replace_ordinary_input_and_are_not_an_additive_charge() {
    let book = PriceBook::bundled().unwrap();
    let mut request = fact(100);
    request.tokens.cached = Some(60);
    request.tokens.cache_write = Some(20);
    request.tokens.output = Some(10);
    request.tokens.total = Some(110);
    assert_eq!(book.price(&request).unwrap().1, 196_000);
    request.tokens.cache_write = Some(41);
    assert!(request.tokens.validate().is_err());
    assert!(book.price(&request).is_none());
    request.tokens.cache_write = None;
    assert!(book.price(&request).is_none());
}

#[test]
fn unknown_tiers_models_providers_and_historical_dates_are_unpriced() {
    let book = PriceBook::bundled().unwrap();
    for tier in [None, Some("auto"), Some("unknown")] {
        let mut request = fact(100);
        request.service_tier = tier.map(str::to_owned);
        assert!(book.price(&request).is_none());
    }
    let mut request = fact(100);
    request.model = "unknown".into();
    assert!(book.price(&request).is_none());
    request.model = "gpt-6.1-sol".into();
    request.provider = "gateway".into();
    assert!(book.price(&request).is_none());
    request.provider = "openai".into();
    request.timestamp = "2026-10-05T23:59:59Z".into();
    assert!(book.price(&request).is_none());
}

#[test]
fn fast_alias_and_fixed_price_history_are_preserved() {
    let book = PriceBook::bundled().unwrap();
    let mut request = fact(100);
    let standard = book.price(&request).unwrap().1;
    request.service_tier = Some("priority".into());
    assert_eq!(book.price(&request).unwrap().1, standard * 2);
    book.apply(&mut request);
    let recorded = (request.price_version.clone(), request.cost_nanousd);
    request.service_tier = Some("standard".into());
    book.apply(&mut request);
    assert_eq!((request.price_version, request.cost_nanousd), recorded);
}

#[test]
fn reference_projection_revalues_history_and_resumes_without_changing_ledger() {
    use pulse_core::{
        domain::SessionMeta,
        ledger::ParserState,
        storage::{FileCursor, Store},
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reference.sqlite");
    let mut store = Store::open(&path).unwrap();
    let book = PriceBook::bundled().unwrap();
    let mut a = fact(100);
    a.timestamp = "2026-09-15T12:00:00Z".into();
    a.cost_nanousd = Some(123);
    a.price_version = Some("historical".into());
    let mut b = a.clone();
    b.id = "unknown".into();
    b.service_tier = None;
    let cursor = FileCursor {
        source_id: "local".into(),
        file_key: "test".into(),
        offset: 1,
        file_identity: "test".into(),
        modified_ns: "1".into(),
        parser_version: pulse_core::PARSER_VERSION,
        state: ParserState::default(),
    };
    store
        .commit_batch(
            &cursor,
            &[SessionMeta {
                id: "session".into(),
                last_activity: a.timestamp.clone(),
                ..Default::default()
            }],
            &[a.clone(), b],
            &[],
            chrono_tz::UTC,
        )
        .unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
    assert!(
        store
            .summary(day, chrono_tz::UTC)
            .unwrap()
            .reference
            .pending
    );
    assert_eq!(store.step_reference_prices(&book, 1).unwrap(), 1);
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(store.step_reference_prices(&book, 1).unwrap(), 1);
    assert_eq!(store.step_reference_prices(&book, 1).unwrap(), 0);
    let summary = store.summary(day, chrono_tz::UTC).unwrap();
    assert_eq!(summary.reference.cost_nanousd, 1_200_000);
    assert_eq!(summary.reference.unpriced_tokens, 200);
    assert_eq!(summary.reference.unpriced_events, 1);
    assert!(!summary.reference.pending);
    assert_eq!(summary.cost_nanousd, 246);
    assert!(book.price(&a).is_none());
    assert_eq!(book.reference_price(&a).unwrap().1, 1_200_000);
    let mut changed = book.clone();
    for price in &mut changed.prices {
        price.output_microusd *= 2;
    }
    assert_eq!(store.step_reference_prices(&changed, 1).unwrap(), 1);
    assert!(
        store
            .summary(day, chrono_tz::UTC)
            .unwrap()
            .reference
            .pending
    );
    store.step_reference_prices(&changed, 1).unwrap();
    assert_eq!(
        store
            .summary(day, chrono_tz::UTC)
            .unwrap()
            .reference
            .cost_nanousd,
        2_200_000
    );
    // Returning to a prior price book must not read old rows beyond the new checkpoint.
    store.step_reference_prices(&book, 1).unwrap();
    let returning = store.summary(day, chrono_tz::UTC).unwrap().reference;
    assert!(returning.pending);
    assert_eq!(returning.cost_nanousd, 1_200_000);
    assert_eq!(returning.unpriced_events, 1);
    assert_eq!(returning.unpriced_tokens, 200);
    store.step_reference_prices(&book, 1).unwrap();
    assert!(
        !store
            .summary(day, chrono_tz::UTC)
            .unwrap()
            .reference
            .pending
    );
    let retained = rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT json FROM usage_facts WHERE id='request'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap();
    assert_eq!(serde_json::from_str::<UsageFact>(&retained).unwrap(), a);
}
