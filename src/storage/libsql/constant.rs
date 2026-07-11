// Environment Variables
pub const ENV_DATABASE_URL: &str = "ASOBI_DATABASE_URL";
pub const ENV_OBSERVATION_LIMIT: &str = "ASOBI_OBSERVATION_LIMIT";
pub const ENV_BUSY_TIMEOUT: &str = "ASOBI_BUSY_TIMEOUT";
pub const ENV_JOURNAL_MODE: &str = "ASOBI_JOURNAL_MODE";

/// Per-entity observation cap when neither `ASOBI_OBSERVATION_LIMIT` nor
/// `asobi.toml` overrides it. Appending past it evicts the oldest rows. Truths
/// are exempt (they upsert), so current state never counts toward this.
pub const DEFAULT_OBSERVATION_LIMIT: usize = 200;

// Pragmas
pub const PRAGMA_FOREIGN_KEYS_ON: &str = "PRAGMA foreign_keys = ON";
pub const PRAGMA_JOURNAL_MODE_WAL: &str = "PRAGMA journal_mode = WAL";
pub const PRAGMA_SYNCHRONOUS_NORMAL: &str = "PRAGMA synchronous = NORMAL";

// Table schema statements
pub const SCHEMA_CREATE_TOPICS: &str = "CREATE TABLE IF NOT EXISTS topics (
            id        TEXT PRIMARY KEY,
            title     TEXT NOT NULL,
            file_path TEXT NOT NULL,
            body      TEXT NOT NULL DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )";

pub const SCHEMA_CREATE_TOPICS_FTS: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS topics_fts
          USING fts5(title, body, content='topics', content_rowid='rowid', tokenize='porter unicode61')";

pub const SCHEMA_CREATE_SESSIONS: &str = "CREATE TABLE IF NOT EXISTS sessions (
            id        TEXT PRIMARY KEY,
            summary   TEXT NOT NULL,
            file_path TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )";

pub const SCHEMA_CREATE_ASOBI_ENTITIES: &str = "CREATE TABLE IF NOT EXISTS asobi_entities (
            name        TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        )";

pub const SCHEMA_CREATE_ASOBI_OBSERVATIONS: &str = "CREATE TABLE IF NOT EXISTS asobi_observations (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            entity_name TEXT NOT NULL,
            content     TEXT NOT NULL,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (entity_name) REFERENCES asobi_entities(name) ON DELETE CASCADE
        )";

pub const SCHEMA_CREATE_IDX_ASOBI_OBSERVATIONS: &str =
    "CREATE INDEX IF NOT EXISTS idx_asobi_observations_entity_name
          ON asobi_observations(entity_name)";

pub const SCHEMA_CREATE_ASOBI_RELATIONS: &str = "CREATE TABLE IF NOT EXISTS asobi_relations (
            from_entity   TEXT NOT NULL,
            to_entity     TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (from_entity, to_entity, relation_type),
            FOREIGN KEY (from_entity) REFERENCES asobi_entities(name) ON DELETE CASCADE,
            FOREIGN KEY (to_entity)   REFERENCES asobi_entities(name) ON DELETE CASCADE
        )";

pub const SCHEMA_CREATE_ASOBI_TRUTHS: &str = "CREATE TABLE IF NOT EXISTS asobi_truths (
            entity_name TEXT NOT NULL,
            key         TEXT NOT NULL,
            value       TEXT NOT NULL,
            updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (entity_name, key),
            FOREIGN KEY (entity_name) REFERENCES asobi_entities(name) ON DELETE CASCADE
        )";

pub const SCHEMA_CREATE_ASOBI_SKILLS: &str = "CREATE TABLE IF NOT EXISTS asobi_skills (
            entity_name  TEXT PRIMARY KEY,
            body         TEXT NOT NULL,
            source       TEXT NOT NULL,
            version      TEXT NOT NULL,
            installed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (entity_name) REFERENCES asobi_entities(name) ON DELETE CASCADE
        )";

pub const SCHEMA_CREATE_CHUNKS: &str = "CREATE TABLE IF NOT EXISTS chunks (
            id        TEXT PRIMARY KEY,
            topic_id  TEXT NOT NULL,
            chunk_idx INTEGER NOT NULL,
            text      TEXT NOT NULL,
            source    TEXT NOT NULL,
            embedding F32_BLOB(384) NOT NULL
        )";

pub const SCHEMA_CREATE_IDX_CHUNKS_TOPIC_ID: &str =
    "CREATE INDEX IF NOT EXISTS idx_chunks_topic_id ON chunks(topic_id)";

pub const SCHEMA_CREATE_IDX_CHUNKS_VECTOR: &str = "CREATE INDEX IF NOT EXISTS idx_chunks_vector ON chunks(libsql_vector_idx(embedding, 'metric=cosine'))";

pub const SCHEMA_CREATE_TRIGGER_TOPICS_AI: &str =
    "CREATE TRIGGER IF NOT EXISTS topics_ai AFTER INSERT ON topics BEGIN
            INSERT INTO topics_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
        END";

pub const SCHEMA_CREATE_TRIGGER_TOPICS_AD: &str = "CREATE TRIGGER IF NOT EXISTS topics_ad AFTER DELETE ON topics BEGIN
            INSERT INTO topics_fts(topics_fts, rowid, title, body) VALUES('delete', old.rowid, old.title, old.body);
        END";

pub const SCHEMA_CREATE_TRIGGER_TOPICS_AU: &str = "CREATE TRIGGER IF NOT EXISTS topics_au AFTER UPDATE ON topics BEGIN
            INSERT INTO topics_fts(topics_fts, rowid, title, body) VALUES('delete', old.rowid, old.title, old.body);
            INSERT INTO topics_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
        END";

// FTS5 external-content table for graph observation search (porter stemming, BM25 ranking).
// content= must reference the current table name, so this is the asobi-native form.
pub const SCHEMA_CREATE_ASOBI_OBS_FTS: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS asobi_obs_fts
          USING fts5(content, content='asobi_observations', content_rowid='rowid', tokenize='porter unicode61')";

pub const SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AI: &str =
    "CREATE TRIGGER IF NOT EXISTS asobi_obs_ai AFTER INSERT ON asobi_observations BEGIN
            INSERT INTO asobi_obs_fts(rowid, content) VALUES (new.rowid, new.content);
        END";

pub const SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AD: &str = "CREATE TRIGGER IF NOT EXISTS asobi_obs_ad AFTER DELETE ON asobi_observations BEGIN
            INSERT INTO asobi_obs_fts(asobi_obs_fts, rowid, content) VALUES('delete', old.rowid, old.content);
        END";

pub const SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AU: &str = "CREATE TRIGGER IF NOT EXISTS asobi_obs_au AFTER UPDATE ON asobi_observations BEGIN
            INSERT INTO asobi_obs_fts(asobi_obs_fts, rowid, content) VALUES('delete', old.rowid, old.content);
            INSERT INTO asobi_obs_fts(rowid, content) VALUES (new.rowid, new.content);
        END";

// Queries - Topics
pub const SQL_SEARCH_FTS: &str = "SELECT t.id, t.title, t.file_path, bm25(topics_fts) AS score
               FROM topics_fts
               JOIN topics t ON topics_fts.rowid = t.rowid
               WHERE topics_fts MATCH ?1
               ORDER BY score
               LIMIT ?2";

pub const SQL_UPSERT_TOPIC: &str = "INSERT INTO topics (id, title, file_path, body) VALUES (?1, ?2, ?3, ?4) \
     ON CONFLICT(id) DO UPDATE SET title=excluded.title, file_path=excluded.file_path, body=excluded.body, updated_at=CURRENT_TIMESTAMP";

// Queries - Asobi Entities / Observations / Relations
pub const SQL_INSERT_ENTITY: &str =
    "INSERT OR IGNORE INTO asobi_entities (name, entity_type) VALUES (?1, ?2)";
pub const SQL_INSERT_OBSERVATION: &str =
    "INSERT INTO asobi_observations (entity_name, content) VALUES (?1, ?2)";
pub const SQL_INSERT_RELATION: &str = "INSERT OR REPLACE INTO asobi_relations (from_entity, to_entity, relation_type) VALUES (?1, ?2, ?3)";
pub const SQL_DELETE_ENTITY: &str = "DELETE FROM asobi_entities WHERE name = ?1";
pub const SQL_DELETE_OBSERVATION: &str =
    "DELETE FROM asobi_observations WHERE entity_name = ?1 AND content = ?2";
pub const SQL_DELETE_OBSERVATION_BY_ID: &str =
    "DELETE FROM asobi_observations WHERE id = ?1 AND entity_name = ?2";
pub const SQL_UPDATE_OBSERVATION: &str =
    "UPDATE asobi_observations SET content = ?3 WHERE entity_name = ?1 AND content = ?2";
pub const SQL_UPDATE_OBSERVATION_BY_ID: &str =
    "UPDATE asobi_observations SET content = ?2 WHERE id = ?1 AND entity_name = ?3";
pub const SQL_DELETE_RELATION: &str =
    "DELETE FROM asobi_relations WHERE from_entity = ?1 AND to_entity = ?2 AND relation_type = ?3";
pub const SQL_EVICT_OBSERVATIONS: &str = "DELETE FROM asobi_observations WHERE entity_name = ?1 AND rowid NOT IN \
     (SELECT rowid FROM asobi_observations WHERE entity_name = ?1 ORDER BY rowid DESC LIMIT ?2)";

pub const SQL_UPSERT_TRUTH: &str = "INSERT INTO asobi_truths (entity_name, key, value) VALUES (?1, ?2, ?3) \
     ON CONFLICT(entity_name, key) DO UPDATE SET value=excluded.value, updated_at=CURRENT_TIMESTAMP";
pub const SQL_DELETE_TRUTH: &str = "DELETE FROM asobi_truths WHERE entity_name = ?1 AND key = ?2";

pub const SQL_UPSERT_SKILL: &str = "INSERT INTO asobi_skills (entity_name, body, source, version) VALUES (?1, ?2, ?3, ?4) \
     ON CONFLICT(entity_name) DO UPDATE SET body=excluded.body, source=excluded.source, version=excluded.version, installed_at=CURRENT_TIMESTAMP";
pub const SQL_SELECT_SKILL_BODY: &str = "SELECT body FROM asobi_skills WHERE entity_name = ?1";
pub const SQL_SELECT_SKILL_BODIES_IN_TEMPLATE: &str =
    "SELECT entity_name, body FROM asobi_skills WHERE entity_name IN ({})";
pub const SQL_LIST_SKILLS: &str = "SELECT s.entity_name, COALESCE(t.value, '') AS description, s.version, s.source, s.installed_at \
     FROM asobi_skills s \
     LEFT JOIN asobi_truths t ON t.entity_name = s.entity_name AND t.key = 'description' \
     ORDER BY s.source, s.entity_name";

pub const SQL_SELECT_ALL_ENTITIES: &str = "SELECT name, entity_type FROM asobi_entities";
pub const SQL_SELECT_ALL_TOPIC_IDS: &str = "SELECT id FROM topics";
pub const SQL_SELECT_OBSERVATIONS_FOR_ENTITY: &str =
    "SELECT content FROM asobi_observations WHERE entity_name = ?1";
pub const SQL_SELECT_ALL_RELATIONS: &str =
    "SELECT from_entity, to_entity, relation_type FROM asobi_relations";

// Graph Search
pub const SQL_SEARCH_OBSERVATIONS_FTS: &str = "SELECT o.entity_name
                   FROM asobi_obs_fts
                   JOIN asobi_observations o ON asobi_obs_fts.rowid = o.rowid
                   WHERE asobi_obs_fts MATCH ?1
                   ORDER BY bm25(asobi_obs_fts)
                   LIMIT ?2";

pub const SQL_SEARCH_ENTITIES_LIKE: &str = "SELECT name FROM asobi_entities
              WHERE name LIKE ?1 OR entity_type LIKE ?1
              ORDER BY name
              LIMIT ?2";

pub const SQL_SELECT_RELATIONS_IN_TEMPLATE: &str = "SELECT from_entity, to_entity, relation_type FROM asobi_relations \
              WHERE from_entity IN ({0}) OR to_entity IN ({0})";

pub const SQL_SELECT_ENTITIES_IN_TEMPLATE: &str =
    "SELECT name, entity_type FROM asobi_entities WHERE name IN ({})";

pub const SQL_SELECT_OBSERVATIONS_IN_TEMPLATE: &str = "SELECT id, entity_name, content FROM asobi_observations \
             WHERE entity_name IN ({}) \
             ORDER BY id";

pub const SQL_SELECT_TRUTHS_FOR_ENTITIES: &str = "SELECT entity_name, key, value FROM asobi_truths \
             WHERE entity_name IN ({}) \
             ORDER BY key";

pub const SQL_COUNT_ENTITIES: &str = "SELECT COUNT(*) FROM asobi_entities";
pub const SQL_COUNT_RELATIONS: &str = "SELECT COUNT(*) FROM asobi_relations";
pub const SQL_COUNT_OBSERVATIONS: &str = "SELECT COUNT(*) FROM asobi_observations";

pub const SQL_DELETE_ALL_RELATIONS: &str = "DELETE FROM asobi_relations";
pub const SQL_DELETE_ALL_OBSERVATIONS: &str = "DELETE FROM asobi_observations";
pub const SQL_DELETE_ALL_ENTITIES: &str = "DELETE FROM asobi_entities";
pub const SQL_DELETE_ALL_CHUNKS: &str = "DELETE FROM chunks";
pub const SQL_DELETE_ALL_TOPICS: &str = "DELETE FROM topics";

// Chunks
pub const SQL_INSERT_CHUNK: &str = "INSERT INTO chunks (id, topic_id, chunk_idx, text, source, embedding) \
             VALUES (?1, ?2, ?3, ?4, ?5, vector32(?6))";

// COALESCE guards against NULL distance: vector_distance_cos is undefined for a
// zero-magnitude vector, and a NULL would panic the f64 column read. Treat it as
// maximally distant (1.0 → similarity 0.0).
pub const SQL_SEARCH_CHUNKS: &str = "SELECT c.id, c.topic_id, c.text, c.source, \
             COALESCE(vector_distance_cos(c.embedding, vector32(?1)), 1.0) AS score \
             FROM vector_top_k('idx_chunks_vector', vector32(?1), ?2) AS v \
             JOIN chunks c ON c.rowid = v.id \
             ORDER BY score";

pub const SQL_DELETE_CHUNKS_BY_TOPIC: &str = "DELETE FROM chunks WHERE topic_id = ?1";

// Backup / restore
// VACUUM INTO cannot bind parameters, so the destination is embedded as an
// escaped SQL string literal at the call site (single quotes doubled).
pub const SQL_VACUUM_INTO_TEMPLATE: &str = "VACUUM INTO '{}'";
pub const SQL_TABLE_EXISTS: &str =
    "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1";
pub const SQL_INTEGRITY_CHECK: &str = "PRAGMA integrity_check";
