-- Rebuildable current API valuation; historical ledger prices stay immutable.
CREATE TABLE reference_prices (
  fact_id TEXT PRIMARY KEY REFERENCES usage_facts(id),
  version TEXT NOT NULL,
  cost_nanousd INTEGER
);
CREATE TABLE reference_price_state (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  version TEXT NOT NULL,
  through_rowid INTEGER NOT NULL
);
INSERT INTO schema_version VALUES (6);
