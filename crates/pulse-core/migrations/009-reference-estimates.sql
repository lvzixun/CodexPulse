-- Only the rebuildable reference projection changes; recorded facts/costs stay intact.
ALTER TABLE reference_prices ADD COLUMN max_cost_nanousd INTEGER;
ALTER TABLE reference_prices ADD COLUMN estimate_flags INTEGER NOT NULL DEFAULT 0;
ALTER TABLE reference_prices ADD COLUMN reason TEXT NOT NULL DEFAULT '';
DROP INDEX reference_range;
DROP INDEX reference_model_range;
DROP INDEX reference_session;
CREATE INDEX reference_range ON reference_prices(version,occurred_at,model,ledger_rowid,total,cost_nanousd,max_cost_nanousd,estimate_flags,reason);
CREATE INDEX reference_model_range ON reference_prices(version,model,occurred_at,ledger_rowid,total,cost_nanousd,max_cost_nanousd,estimate_flags,reason);
CREATE INDEX reference_session ON reference_prices(version,session_id,ledger_rowid,total,cost_nanousd,max_cost_nanousd,estimate_flags,reason);
INSERT INTO schema_version VALUES (9);
