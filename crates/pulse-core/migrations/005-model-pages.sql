-- Model pages aggregate normalized columns without fetching large fact JSON rows.
CREATE INDEX facts_time_model_counts ON usage_facts(occurred_at,model,session_id,total,input,cached,output,cost_nanousd);
INSERT INTO schema_version VALUES (5);
