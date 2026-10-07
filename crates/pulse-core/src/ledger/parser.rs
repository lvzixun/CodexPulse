use crate::domain::{OutputRate, ParseIssue, SessionMeta, TokenCounts, UsageFact};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParserState {
    /// Reader recovery state; oversized records are discarded without allocating their body.
    #[serde(default)]
    pub discarding_line: bool,
    #[serde(default)]
    pub observed_file_len: Option<u64>,
    pub session: Option<SessionMeta>,
    #[serde(default)]
    pub session_source_known: bool,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub turn_id: Option<String>,
    pub service_tier: Option<String>,
    pub cumulative: Option<TokenCounts>,
    pub epoch: u64,
    pub ordinal: u64,
    pub inherited_until: Option<u64>,
    pub inherited: bool,
    #[serde(default)]
    pub rate_started_at: Option<String>,
    #[serde(default)]
    pub rate_output: Option<u64>,
}

#[derive(Debug, Default)]
pub struct ParseOutput {
    pub fact: Option<UsageFact>,
    pub session: Option<SessionMeta>,
    pub issue: Option<ParseIssue>,
    /// A log quota observation has no retroactive account identity.
    pub quota_observation: Option<Value>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
fn source_kind(payload: &Value) -> Option<String> {
    if payload["source"]
        .get("subagent")
        .is_some_and(|v| !v.is_null())
    {
        Some("subagent".into())
    } else {
        text(payload, "source").or_else(|| text(payload, "originator"))
    }
}
fn counters(value: &Value) -> Option<TokenCounts> {
    if !value.is_object() {
        return None;
    }
    let count = TokenCounts {
        input: value.get("input_tokens").and_then(Value::as_u64),
        cached: value.get("cached_input_tokens").and_then(Value::as_u64),
        output: value.get("output_tokens").and_then(Value::as_u64),
        reasoning: value.get("reasoning_output_tokens").and_then(Value::as_u64),
        cache_write: value
            .get("cache_write_input_tokens")
            .and_then(Value::as_u64),
        total: value.get("total_tokens").and_then(Value::as_u64),
    };
    count.effective_total()?;
    Some(count)
}

impl ParserState {
    fn set_model(&mut self, model: Option<String>) {
        if self.model != model {
            if let Some(session) = self.session.as_mut() {
                session.output_rate = None;
            }
            // task_started can precede the first model context. Keep its fresh
            // clock until output arrives, but never attribute old output to a new model.
            if self.rate_output != Some(0) {
                self.rate_started_at = None;
                self.rate_output = None;
            }
        }
        self.model = model;
        if let Some(session) = self.session.as_mut() {
            session.current_model = self.model.clone();
        }
    }
    /// Upgrade old checkpoints from the bounded metadata header, without replaying
    /// counters or resetting the current activity/output-rate state.
    pub fn recover_session_source(&mut self, raw: &[u8]) -> Option<SessionMeta> {
        self.session_source_known = true;
        let record: Value = serde_json::from_slice(raw).ok()?;
        if record["type"] != "session_meta" {
            return None;
        }
        let payload = &record["payload"];
        let id = text(payload, "id").or_else(|| text(payload, "session_id"))?;
        let session = self.session.as_mut()?;
        if session.id != id {
            return None;
        }
        session.source_kind = source_kind(payload);
        Some(session.clone())
    }
    pub fn parse(&mut self, raw: &[u8]) -> ParseOutput {
        self.ordinal += 1;
        let record: Value = match serde_json::from_slice(raw) {
            Ok(v) => v,
            Err(_) => return Self::issue("invalid_json", None),
        };
        let payload = &record["payload"];
        let timestamp = text(&record, "timestamp");
        let kind = record["type"].as_str().unwrap_or_default();
        if kind == "session_meta" {
            let Some(id) = text(payload, "id").or_else(|| text(payload, "session_id")) else {
                return Self::issue("missing_session_id", timestamp);
            };
            if self.session.as_ref().is_some_and(|s| s.id != id) {
                *self = Self {
                    ordinal: self.ordinal,
                    ..Self::default()
                };
            }
            self.provider = text(payload, "model_provider");
            let parent = text(payload, "forked_from_id")
                .or_else(|| text(&payload["history_base"], "thread_id"));
            self.inherited = parent.is_some();
            self.inherited_until = payload
                .get("subagent_history_start_ordinal")
                .and_then(Value::as_u64);
            let project = text(payload, "cwd").and_then(|cwd| {
                cwd.trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
                    .map(str::to_owned)
            });
            self.session = Some(SessionMeta {
                id,
                title: text(payload, "name").or_else(|| text(payload, "title")),
                project,
                source_kind: source_kind(payload),
                source_version: text(payload, "cli_version"),
                parent_id: parent,
                status: "unknown".into(),
                last_activity: timestamp.clone().unwrap_or_default(),
                current_model: self.model.clone(),
                output_rate: None,
            });
            self.session_source_known = true;
            return ParseOutput {
                session: self.session.clone(),
                ..Default::default()
            };
        }
        if kind == "turn_context" {
            if self.model != text(payload, "model") {
                self.service_tier = None;
            }
            let next_model = text(payload, "model");
            self.set_model(next_model);
            if let Some(provider) = text(payload, "model_provider") {
                self.provider = Some(provider);
            }
            self.turn_id = text(payload, "turn_id");
            if payload.get("service_tier").is_some() {
                self.service_tier = text(payload, "service_tier");
            }
            return ParseOutput {
                session: self.session.clone(),
                ..Default::default()
            };
        }
        if kind != "event_msg" {
            return ParseOutput::default();
        }
        match payload["type"].as_str().unwrap_or_default() {
            "thread_settings_applied" => {
                // Copied history retains its logical owner; it must not change this thread's settings.
                if text(payload, "thread_id").is_some_and(|owner| {
                    self.session
                        .as_ref()
                        .is_none_or(|session| session.id != owner)
                }) {
                    return ParseOutput::default();
                }
                let settings = &payload["thread_settings"];
                let next_model = text(settings, "model");
                self.set_model(next_model);
                if let Some(provider) = text(settings, "model_provider_id") {
                    self.provider = Some(provider);
                }
                self.service_tier = text(settings, "service_tier");
                ParseOutput {
                    session: self.session.clone(),
                    ..Default::default()
                }
            }
            "task_started" | "turn_started" => {
                self.turn_id = text(payload, "turn_id");
                self.rate_started_at = timestamp.clone();
                self.rate_output = Some(0);
                if let Some(session) = self.session.as_mut() {
                    session.output_rate = None;
                }
                self.set_activity("active", timestamp.as_deref());
                ParseOutput {
                    session: self.session.clone(),
                    ..Default::default()
                }
            }
            "task_complete" | "turn_completed" => {
                if text(payload, "turn_id") == self.turn_id {
                    self.update_rate(timestamp.as_deref(), payload["duration_ms"].as_u64(), true);
                }
                self.rate_started_at = None;
                self.set_activity("completed", timestamp.as_deref());
                ParseOutput {
                    session: self.session.clone(),
                    ..Default::default()
                }
            }
            "turn_aborted" => {
                self.rate_started_at = None;
                if let Some(session) = self.session.as_mut() {
                    session.output_rate = None;
                }
                self.set_activity("interrupted", timestamp.as_deref());
                ParseOutput {
                    session: self.session.clone(),
                    ..Default::default()
                }
            }
            "token_count" => self.parse_usage(&record, payload, timestamp),
            _ => ParseOutput::default(),
        }
    }

    fn set_activity(&mut self, status: &str, timestamp: Option<&str>) {
        if let Some(session) = self.session.as_mut() {
            session.status = status.into();
            if let Some(ts) = timestamp {
                session.last_activity = ts.into();
            }
        }
    }
    fn update_rate(&mut self, timestamp: Option<&str>, duration: Option<u64>, completed: bool) {
        let sample = (|| {
            let start = DateTime::parse_from_rfc3339(self.rate_started_at.as_deref()?).ok()?;
            let end_text = timestamp?;
            let end = DateTime::parse_from_rfc3339(end_text).ok()?;
            let observed = u64::try_from((end - start).num_milliseconds()).ok()?;
            if observed == 0 {
                return None;
            }
            // Completion duration is supplied by the client. Reject contradictory timing.
            let elapsed_ms = duration.unwrap_or(observed);
            if elapsed_ms == 0 || elapsed_ms.abs_diff(observed) > 2000 {
                return None;
            }
            let output_tokens = self.rate_output.filter(|n| *n > 0)?;
            Some(OutputRate {
                output_tokens,
                elapsed_ms,
                measured_at: end_text.into(),
                completed,
            })
        })();
        if let Some(session) = self.session.as_mut() {
            session.output_rate = sample;
        }
    }
    fn issue(code: &str, timestamp: Option<String>) -> ParseOutput {
        ParseOutput {
            issue: Some(ParseIssue {
                code: code.into(),
                timestamp,
            }),
            ..Default::default()
        }
    }
    fn parse_usage(
        &mut self,
        record: &Value,
        payload: &Value,
        timestamp: Option<String>,
    ) -> ParseOutput {
        let quota = payload
            .get("rate_limits")
            .filter(|v| v.is_object())
            .cloned();
        let info = &payload["info"];
        if info.is_null() {
            return ParseOutput {
                quota_observation: quota,
                ..Default::default()
            };
        }
        let Some(session_id) = self.session.as_ref().map(|s| s.id.clone()) else {
            return Self::issue("usage_without_session", timestamp);
        };
        let Some(ts) = timestamp
            .as_ref()
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|ts| {
                ts.with_timezone(&chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
            })
        else {
            return Self::issue("invalid_usage_time", timestamp);
        };
        for name in ["total_token_usage", "last_token_usage"] {
            for field in [
                "input_tokens",
                "cached_input_tokens",
                "output_tokens",
                "reasoning_output_tokens",
                "cache_write_input_tokens",
                "total_tokens",
            ] {
                if info[name]
                    .get(field)
                    .is_some_and(|v| !v.is_null() && v.as_u64().is_none())
                {
                    return Self::issue("invalid_token_counters", timestamp);
                }
            }
        }
        let total = counters(&info["total_token_usage"]);
        let last = counters(&info["last_token_usage"]);
        let mut quality = "complete";
        let owned = match (&total, &self.cumulative) {
            (Some(now), Some(previous)) => {
                if now.validate().is_err() {
                    return Self::issue("invalid_token_counters", timestamp);
                }
                if let Some(delta) = now.checked_delta(previous) {
                    Some(delta)
                } else {
                    // A correction may replay an old last-usage record. Establish a new baseline;
                    // never invent ownership for this event from a decreasing cumulative snapshot.
                    self.epoch += 1;
                    quality = "counter_discontinuity";
                    None
                }
            }
            (Some(now), None) => {
                if now.validate().is_err() {
                    return Self::issue("invalid_token_counters", timestamp);
                }
                if self.inherited {
                    last.clone().filter(|l| now.checked_delta(l).is_some())
                } else {
                    Some(now.clone())
                }
            }
            (None, _) => last.clone(),
        };
        if let Some(now) = total {
            self.cumulative = Some(now);
        }
        if self.inherited_until.is_some_and(|end| self.ordinal < end) {
            return ParseOutput {
                quota_observation: quota,
                ..Default::default()
            };
        }
        let Some(tokens) = owned else {
            self.rate_output = None;
            self.update_rate(Some(&ts), None, false);
            let mut result = Self::issue("usage_ownership_unresolved", timestamp);
            result.quota_observation = quota;
            return result;
        };
        if tokens.validate().is_err() {
            return Self::issue("invalid_token_counters", timestamp);
        }
        if tokens.is_zero() {
            return ParseOutput {
                quota_observation: quota,
                ..Default::default()
            };
        }
        if tokens.input.is_none() || tokens.output.is_none() {
            quality = "unsplit";
        }
        if self.rate_started_at.is_some() {
            self.rate_output = self
                .rate_output
                .zip(tokens.output)
                .and_then(|(a, b)| a.checked_add(b));
            self.update_rate(Some(&ts), None, false);
        }
        let fingerprint = serde_json::to_vec(&(
            1,
            &session_id,
            &self.turn_id,
            &ts,
            self.epoch,
            &record["payload"]["info"],
        ))
        .expect("serializable token data");
        let id = format!("{:x}", Sha256::digest(fingerprint));
        let model = self.model.clone().unwrap_or_else(|| "unknown".into());
        let provider = self.provider.clone().unwrap_or_else(|| "unknown".into());
        let request_input = last
            .as_ref()
            .filter(|request| request.validate().is_ok() && *request == &tokens)
            .and_then(|request| request.input);
        if let Some(session) = self.session.as_mut() {
            session.last_activity = ts.clone();
        }
        ParseOutput {
            fact: Some(UsageFact {
                id,
                session_id,
                turn_id: self.turn_id.clone(),
                model,
                provider,
                timestamp: ts,
                tokens,
                service_tier: self.service_tier.clone(),
                request_input,
                quality: quality.into(),
                price_version: None,
                cost_nanousd: None,
            }),
            session: self.session.clone(),
            quota_observation: quota,
            issue: None,
        }
    }
}
