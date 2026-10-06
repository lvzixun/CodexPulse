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
