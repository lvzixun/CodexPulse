use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaWindow {
    pub remaining_percent: Option<f64>,
    pub duration_minutes: Option<u64>,
    pub resets_at: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaBucket {
    pub source_id: String,
    pub identity_key: String,
    pub identity_confirmed: bool,
    pub limit_id: String,
    pub name: String,
    pub plan: Option<String>,
    pub primary: Option<QuotaWindow>,
    pub secondary: Option<QuotaWindow>,
    pub captured_at: String,
}
pub fn normalize(
    source: &str,
    account: &Value,
    response: &Value,
    captured_at: &str,
) -> Vec<QuotaBucket> {
    let account_id = response["accountId"].as_str().filter(|s| !s.is_empty());
    let workspace = account["workspaceRouting"]["chatgptAccountId"]
        .as_str()
        .filter(|s| !s.is_empty());
    let origin = account["workspaceRouting"]["backendOrigin"]
        .as_str()
        .filter(|s| !s.is_empty());
    let confirmed = account_id.is_some() && workspace.is_some() && origin.is_some();
    let identity = if confirmed {
        serde_json::to_vec(&(account_id, workspace, origin)).unwrap()
    } else {
        serde_json::to_vec(&("source", source)).unwrap()
    };
    let identity_key = format!("{:x}", Sha256::digest(identity));
    let buckets: Vec<(String, &Value)> = match response["rateLimitsByLimitId"]
        .as_object()
        .filter(|m| !m.is_empty())
    {
        Some(map) => map
            .iter()
            .map(|(key, value)| (key.clone(), value))
            .collect(),
        None => {
            if response["rateLimits"].is_object() {
                vec![(
                    response["rateLimits"]["limitId"]
                        .as_str()
                        .unwrap_or("legacy")
                        .into(),
                    &response["rateLimits"],
                )]
            } else {
                Vec::new()
            }
        }
    };
    buckets
        .into_iter()
        .map(|(id, bucket)| QuotaBucket {
            source_id: source.into(),
            identity_key: identity_key.clone(),
            identity_confirmed: confirmed,
            limit_id: bucket["limitId"].as_str().unwrap_or(&id).into(),
            name: bucket["limitName"].as_str().unwrap_or(&id).into(),
            plan: bucket["planType"].as_str().map(str::to_owned),
            primary: window(&bucket["primary"]),
            secondary: window(&bucket["secondary"]),
            captured_at: captured_at.into(),
        })
        .collect()
}
fn window(value: &Value) -> Option<QuotaWindow> {
    if !value.is_object() {
        return None;
    }
    Some(QuotaWindow {
        remaining_percent: value["usedPercent"]
            .as_f64()
            .filter(|v| v.is_finite())
            .map(|v| (100.0 - v).clamp(0.0, 100.0)),
        duration_minutes: value["windowDurationMins"].as_u64().filter(|v| *v > 0),
        resets_at: value["resetsAt"]
            .as_i64()
            .filter(|s| chrono::DateTime::from_timestamp(*s, 0).is_some()),
    })
}
/// Same confirmed account/workspace/plan/bucket uses the freshest snapshot, never adds percentages.
pub fn merged(mut buckets: Vec<QuotaBucket>) -> Vec<QuotaBucket> {
    buckets.sort_by(|a, b| b.captured_at.cmp(&a.captured_at));
    let mut keys = std::collections::HashSet::new();
    buckets.retain(|b| keys.insert((b.identity_key.clone(), b.limit_id.clone(), b.plan.clone())));
    buckets
}
