-- Asobi schema baseline (schema 9, ADR 0007), carried to sqlx migrations by ADR 0008.
-- Version tracking moved from PRAGMA user_version to sqlx's _sqlx_migrations table;
-- every statement is IF NOT EXISTS so running it on an already-current file is a no-op.

CREATE TABLE IF NOT EXISTS asobi_entities (
    name TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_activity TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS asobi_observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_name TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_observations_entity ON asobi_observations(entity_name);
CREATE TABLE IF NOT EXISTS asobi_relations (
    from_entity TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
    to_entity TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (from_entity, to_entity, relation_type)
);
CREATE INDEX IF NOT EXISTS idx_relations_to ON asobi_relations(to_entity, from_entity, relation_type);
CREATE TABLE IF NOT EXISTS asobi_truths (
    entity_name TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (entity_name, key)
);
CREATE INDEX IF NOT EXISTS idx_truths_lookup ON asobi_truths(key, value, entity_name);
CREATE VIRTUAL TABLE IF NOT EXISTS asobi_truth_fts USING fts5(
    value, content='asobi_truths', content_rowid='rowid',
    tokenize='porter unicode61'
);
CREATE TRIGGER IF NOT EXISTS asobi_truth_ai AFTER INSERT ON asobi_truths BEGIN
    INSERT INTO asobi_truth_fts(rowid, value) VALUES (new.rowid, new.value);
END;
CREATE TRIGGER IF NOT EXISTS asobi_truth_ad AFTER DELETE ON asobi_truths BEGIN
    INSERT INTO asobi_truth_fts(asobi_truth_fts, rowid, value) VALUES ('delete', old.rowid, old.value);
END;
CREATE TRIGGER IF NOT EXISTS asobi_truth_au AFTER UPDATE ON asobi_truths BEGIN
    INSERT INTO asobi_truth_fts(asobi_truth_fts, rowid, value) VALUES ('delete', old.rowid, old.value);
    INSERT INTO asobi_truth_fts(rowid, value) VALUES (new.rowid, new.value);
END;
CREATE VIRTUAL TABLE IF NOT EXISTS asobi_obs_fts USING fts5(
    content, content='asobi_observations', content_rowid='rowid',
    tokenize='porter unicode61'
);
CREATE TRIGGER IF NOT EXISTS asobi_obs_ai AFTER INSERT ON asobi_observations BEGIN
    INSERT INTO asobi_obs_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER IF NOT EXISTS asobi_obs_ad AFTER DELETE ON asobi_observations BEGIN
    INSERT INTO asobi_obs_fts(asobi_obs_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER IF NOT EXISTS asobi_obs_au AFTER UPDATE ON asobi_observations BEGIN
    INSERT INTO asobi_obs_fts(asobi_obs_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
    INSERT INTO asobi_obs_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER IF NOT EXISTS asobi_activity_obs_ins AFTER INSERT ON asobi_observations BEGIN
    UPDATE asobi_entities SET last_activity = CURRENT_TIMESTAMP WHERE name = new.entity_name;
END;
CREATE TRIGGER IF NOT EXISTS asobi_activity_obs_upd AFTER UPDATE ON asobi_observations BEGIN
    UPDATE asobi_entities SET last_activity = CURRENT_TIMESTAMP WHERE name = new.entity_name;
END;
CREATE TRIGGER IF NOT EXISTS asobi_activity_obs_del AFTER DELETE ON asobi_observations BEGIN
    UPDATE asobi_entities SET last_activity = CURRENT_TIMESTAMP WHERE name = old.entity_name;
END;
CREATE TRIGGER IF NOT EXISTS asobi_activity_truth_ins AFTER INSERT ON asobi_truths BEGIN
    UPDATE asobi_entities SET last_activity = CURRENT_TIMESTAMP WHERE name = new.entity_name;
END;
CREATE TRIGGER IF NOT EXISTS asobi_activity_truth_upd AFTER UPDATE ON asobi_truths BEGIN
    UPDATE asobi_entities SET last_activity = CURRENT_TIMESTAMP WHERE name = new.entity_name;
END;
CREATE TRIGGER IF NOT EXISTS asobi_activity_truth_del AFTER DELETE ON asobi_truths BEGIN
    UPDATE asobi_entities SET last_activity = CURRENT_TIMESTAMP WHERE name = old.entity_name;
END;
