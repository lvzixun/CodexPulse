-- SQLite promotes overflowing integer additions to REAL. Refuse that promotion so
-- an aggregate failure rolls back the facts and checkpoint in the same transaction.
CREATE TRIGGER daily_usage_insert_guard BEFORE INSERT ON daily_model_usage
WHEN typeof(NEW.total)!='integer' OR NEW.total<0 OR typeof(NEW.input)!='integer' OR NEW.input<0 OR typeof(NEW.cached)!='integer' OR NEW.cached<0 OR typeof(NEW.output)!='integer' OR NEW.output<0 OR typeof(NEW.cost_nanousd)!='integer' OR NEW.cost_nanousd<0 OR typeof(NEW.unpriced_tokens)!='integer' OR NEW.unpriced_tokens<0 OR typeof(NEW.incomplete_events)!='integer' OR NEW.incomplete_events<0
BEGIN SELECT RAISE(ABORT,'usage aggregate overflow'); END;
CREATE TRIGGER daily_usage_update_guard BEFORE UPDATE ON daily_model_usage
WHEN typeof(NEW.total)!='integer' OR NEW.total<0 OR typeof(NEW.input)!='integer' OR NEW.input<0 OR typeof(NEW.cached)!='integer' OR NEW.cached<0 OR typeof(NEW.output)!='integer' OR NEW.output<0 OR typeof(NEW.cost_nanousd)!='integer' OR NEW.cost_nanousd<0 OR typeof(NEW.unpriced_tokens)!='integer' OR NEW.unpriced_tokens<0 OR typeof(NEW.incomplete_events)!='integer' OR NEW.incomplete_events<0
BEGIN SELECT RAISE(ABORT,'usage aggregate overflow'); END;
CREATE TRIGGER session_usage_insert_guard BEFORE INSERT ON session_model_usage
WHEN typeof(NEW.total)!='integer' OR NEW.total<0 OR typeof(NEW.input)!='integer' OR NEW.input<0 OR typeof(NEW.cached)!='integer' OR NEW.cached<0 OR typeof(NEW.output)!='integer' OR NEW.output<0 OR typeof(NEW.cost_nanousd)!='integer' OR NEW.cost_nanousd<0 OR typeof(NEW.unpriced_tokens)!='integer' OR NEW.unpriced_tokens<0
BEGIN SELECT RAISE(ABORT,'usage aggregate overflow'); END;
CREATE TRIGGER session_usage_update_guard BEFORE UPDATE ON session_model_usage
WHEN typeof(NEW.total)!='integer' OR NEW.total<0 OR typeof(NEW.input)!='integer' OR NEW.input<0 OR typeof(NEW.cached)!='integer' OR NEW.cached<0 OR typeof(NEW.output)!='integer' OR NEW.output<0 OR typeof(NEW.cost_nanousd)!='integer' OR NEW.cost_nanousd<0 OR typeof(NEW.unpriced_tokens)!='integer' OR NEW.unpriced_tokens<0
BEGIN SELECT RAISE(ABORT,'usage aggregate overflow'); END;
INSERT INTO schema_version VALUES (3);
