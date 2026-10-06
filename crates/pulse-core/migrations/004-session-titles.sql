-- Titles are independent of rollout activity: a later usage record must not undo a rename.
CREATE TABLE session_titles (
  session_id TEXT PRIMARY KEY,
  title TEXT,
  updated_at TEXT NOT NULL,
  source_id TEXT NOT NULL
);
INSERT INTO schema_version VALUES (4);
