-- Separate from the immutable usage ledger; replay must never reprice or recount it.
CREATE TABLE run_speeds (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  model TEXT NOT NULL,
  started_at TEXT NOT NULL,
  measured_at TEXT NOT NULL,
  output_tokens INTEGER NOT NULL CHECK(output_tokens > 0),
  elapsed_ms INTEGER NOT NULL CHECK(elapsed_ms > 0),
  service_tier TEXT,
  completed INTEGER NOT NULL CHECK(completed IN (0,1))
);
CREATE INDEX run_speed_model_time ON run_speeds(model,completed,measured_at DESC,id DESC);
INSERT INTO schema_version VALUES(8);
