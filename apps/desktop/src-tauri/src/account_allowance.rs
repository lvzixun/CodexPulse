use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountAllowance {
    pub balance: Option<f64>,
    pub unlimited: bool,
    pub reset_cards: Option<u64>,
    pub applicable_reset_cards: Option<u64>,
    pub next_expiration: Option<String>,
    pub next_expiring_count: Option<u64>,
    pub expiration_status: String,
}

fn count(value: &Value) -> Option<u64> {
    value.as_u64().filter(|n| *n <= 9_007_199_254_740_991)
}

pub fn usage(value: &Value) -> AccountAllowance {
    let credits = &value["credits"];
    let balance = credits["balance"]
        .as_f64()
        .or_else(|| {
            credits["balance"]
                .as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 64
                        && s.bytes()
                            .all(|c| c.is_ascii_digit() || c == b'.' || c == b'-')
                })
                .and_then(|s| s.parse().ok())
        })
        .filter(|n| n.is_finite() && n.abs() <= 9_007_199_254_740_991.0);
    AccountAllowance {
        balance,
        unlimited: credits["unlimited"].as_bool() == Some(true),
        reset_cards: count(&value["rate_limit_reset_credits"]["available_count"]),
        applicable_reset_cards: count(
            &value["rate_limit_reset_credits"]["applicable_available_count"],
        ),
        expiration_status: "awaiting_refresh".into(),
        ..Default::default()
    }
}

impl AccountAllowance {
    pub fn apply_cards(
        &mut self,
        value: &Value,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), &'static str> {
        let cards = value["credits"].as_array().ok_or("unsupported_response")?;
        let available = count(&value["available_count"]).ok_or("unsupported_response")?;
        let mut earliest = None;
        let mut expiring = 0;
        for card in cards
            .iter()
            .filter(|c| c["status"].as_str() == Some("available"))
        {
            let Some(expiration) = card["expires_at"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&chrono::Utc))
                .filter(|d| *d > now)
            else {
                continue;
            };
            match earliest {
                None => {
                    earliest = Some(expiration);
                    expiring = 1;
                }
                Some(date) if expiration < date => {
                    earliest = Some(expiration);
                    expiring = 1;
                }
                Some(date) if expiration == date => expiring += 1,
                _ => {}
            }
        }
        self.reset_cards = Some(available);
        self.next_expiration = earliest.map(|d| d.to_rfc3339());
        self.next_expiring_count = earliest.map(|_| expiring);
        self.expiration_status = "connected".into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn credits_keep_units_and_unknown_fields_are_not_zero() {
        let a = usage(
            &json!({"credits":{"balance":"42.75","unlimited":false},"rate_limit_reset_credits":{"available_count":3,"applicable_available_count":2}}),
        );
        assert_eq!(a.balance, Some(42.75));
        assert_eq!(a.reset_cards, Some(3));
        assert_eq!(a.applicable_reset_cards, Some(2));
        assert!(usage(&json!({})).balance.is_none());
        for invalid in [json!("NaN"), json!(""), json!("$20"), json!("1e10")] {
            assert!(
                usage(&json!({"credits":{"balance":invalid}}))
                    .balance
                    .is_none()
            );
        }
        assert_eq!(
            usage(&json!({"credits":{"balance":"0"}})).balance,
            Some(0.0)
        );
        assert_eq!(
            usage(&json!({"credits":{"balance":"-1.25"}})).balance,
            Some(-1.25)
        );
    }
    #[test]
    fn expiry_uses_available_future_cards_and_groups_same_instant() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-06T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut a = usage(&json!({}));
        a.apply_cards(
            &json!({"available_count":4,"credits":[
                {"id":"private-card","status":"available","expires_at":"2026-10-09T01:00:00Z"},
                {"status":"available","expires_at":"2026-10-09T09:00:00+08:00"},
                {"status":"redeemed","expires_at":"2026-10-07T00:00:00Z"},
                {"status":"available","expires_at":"2026-10-05T00:00:00Z"},
                {"status":"available","expires_at":"bad-date"},
                {"status":"available","expires_at":"2026-11-01T00:00:00Z"}
            ]}),
            now,
        )
        .unwrap();
        assert_eq!(
            a.next_expiration.as_deref(),
            Some("2026-10-09T01:00:00+00:00")
        );
        assert_eq!(a.next_expiring_count, Some(2));
        assert!(!serde_json::to_string(&a).unwrap().contains("private-card"));
        assert!(a.apply_cards(&json!({"available_count":2}), now).is_err());
        a.apply_cards(&json!({"available_count":0,"credits":[]}), now)
            .unwrap();
        assert_eq!(a.reset_cards, Some(0));
        assert!(a.next_expiration.is_none());
    }
}
