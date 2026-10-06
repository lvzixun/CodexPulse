-- Keep recorded costs/checkpoints; copy normalized dimensions for direct range sums.
-- The ledger JSON is never rewritten or decoded by this migration.
CREATE TABLE reference_prices_next (
  fact_id TEXT PRIMARY KEY REFERENCES usage_facts(id),
  version TEXT NOT NULL,
  cost_nanousd INTEGER,
  occurred_at TEXT NOT NULL,
  session_id TEXT NOT NULL,
  model TEXT NOT NULL,
  total INTEGER,
  ledger_rowid INTEGER NOT NULL
);
INSERT INTO reference_prices_next
  SELECT p.fact_id,p.version,p.cost_nanousd,f.occurred_at,f.session_id,f.model,f.total,f.rowid
  FROM reference_prices p JOIN usage_facts f ON f.id=p.fact_id;
DROP TABLE reference_prices;
ALTER TABLE reference_prices_next RENAME TO reference_prices;
CREATE INDEX reference_range ON reference_prices(version,occurred_at,model,ledger_rowid,total,cost_nanousd);
CREATE INDEX reference_model_range ON reference_prices(version,model,occurred_at,ledger_rowid,total,cost_nanousd);
CREATE INDEX reference_session ON reference_prices(version,session_id,ledger_rowid,total,cost_nanousd);
INSERT INTO schema_version VALUES (7);
