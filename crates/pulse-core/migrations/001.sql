PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);
INSERT OR IGNORE INTO schema_version VALUES (1);
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  metadata TEXT NOT NULL,
  last_activity TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_activity ON sessions(last_activity DESC, id DESC);
CREATE TABLE IF NOT EXISTS source_sessions (
  source_id TEXT NOT NULL,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  PRIMARY KEY(source_id, session_id)
);
CREATE TABLE IF NOT EXISTS usage_facts (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  model TEXT NOT NULL,
  occurred_at TEXT NOT NULL,
  total INTEGER,
  input INTEGER,
  cached INTEGER,
  output INTEGER,
  cost_nanousd INTEGER,
  json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS facts_time ON usage_facts(occurred_at);
CREATE INDEX IF NOT EXISTS facts_session_model ON usage_facts(session_id, model, occurred_at);
CREATE TABLE IF NOT EXISTS file_cursors (
  source_id TEXT NOT NULL,
  file_key TEXT NOT NULL,
  offset INTEGER NOT NULL,
  file_identity TEXT NOT NULL,
  modified_ns TEXT NOT NULL,
  parser_version INTEGER NOT NULL,
  parser_state TEXT NOT NULL,
  PRIMARY KEY(source_id, file_key)
);
CREATE TABLE IF NOT EXISTS daily_model_usage (
  day TEXT NOT NULL,
  timezone TEXT NOT NULL,
  model TEXT NOT NULL,
  total INTEGER NOT NULL DEFAULT 0,
  input INTEGER NOT NULL DEFAULT 0,
  cached INTEGER NOT NULL DEFAULT 0,
  output INTEGER NOT NULL DEFAULT 0,
  cost_nanousd INTEGER NOT NULL DEFAULT 0,
  unpriced_tokens INTEGER NOT NULL DEFAULT 0,
  incomplete_events INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(day, timezone, model)
);
CREATE TABLE IF NOT EXISTS session_model_usage (
  session_id TEXT NOT NULL REFERENCES sessions(id),
  model TEXT NOT NULL,
  total INTEGER NOT NULL DEFAULT 0,
  input INTEGER NOT NULL DEFAULT 0,
  cached INTEGER NOT NULL DEFAULT 0,
  output INTEGER NOT NULL DEFAULT 0,
  cost_nanousd INTEGER NOT NULL DEFAULT 0,
  unpriced_tokens INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY(session_id, model)
);
CREATE TABLE IF NOT EXISTS parse_issues (
  source_id TEXT NOT NULL,
  file_key TEXT NOT NULL,
  code TEXT NOT NULL,
  count INTEGER NOT NULL DEFAULT 1,
  PRIMARY KEY(source_id, file_key, code)
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, json TEXT NOT NULL);
