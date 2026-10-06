-- DISTINCT session counts must read compact covering indexes, not fact JSON pages.
CREATE INDEX IF NOT EXISTS facts_model_time_session ON usage_facts(model, occurred_at, session_id);
CREATE INDEX IF NOT EXISTS facts_time_session ON usage_facts(occurred_at, session_id);
DROP INDEX IF EXISTS facts_time;
CREATE INDEX IF NOT EXISTS source_sessions_session ON source_sessions(session_id, source_id);
INSERT OR IGNORE INTO schema_version VALUES (2);
