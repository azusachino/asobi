# Specification: Sequential Observation IDs & Agent Precision

**Date**: 2026-07-01  
**Status**: Implemented (v0.3.0)  

---

## 1. Problem Statement

Asobi is designed to act as a project-local persistent knowledge graph CLI for LLM agents. To manage observations under an entity, agents previously matched observations by their exact string content for updates and deletions:

```bash
asobi rm-obs my-project "next: implement FTS5 index"
```

This model had three major drawbacks:
1. **High Token Overhead (Write/Delete Amplification)**: An agent wanting to edit or delete an observation had to copy and send the entire observation text string over command line arguments. For large logs, this resulted in massive token consumption.
2. **Ambiguity**: If two observations under the same entity had the same content string, updating or deleting one would unintentionally modify or delete both.
3. **Database Lookups**: Deletion required full-text scans or string indexing on the `content` field rather than $O(1)$ primary key indexing.

---

## 2. Solution: Sequential Observation IDs

We transition `asobi_observations` to use an `INTEGER PRIMARY KEY AUTOINCREMENT` for the observation identifier. 

### 2.1 Database Schema
The schema for `asobi_observations` is updated to:

```sql
CREATE TABLE asobi_observations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_name TEXT NOT NULL,
    content     TEXT NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (entity_name) REFERENCES asobi_entities(name) ON DELETE CASCADE
);
```

Since the `id` column is an alias for SQLite's 64-bit integer `rowid`, FTS5 triggers that reference `new.rowid` and `old.rowid` remain perfectly functional without schema logic changes.

### 2.2 Eager detailed representation
When an agent or user requests detailed entity views via `asobi show --with-timestamps`, observations are serialized inside an `observationsDetailed` JSON field. Each observation item contains its unique integer `id`:

```json
{
  "entities": [
    {
      "name": "alice",
      "entityType": "person",
      "observations": [
        "status: active",
        "next: code"
      ],
      "observationsDetailed": [
        { "id": 1, "content": "status: active", "createdAt": "2026-07-01 23:00:00" },
        { "id": 2, "content": "next: code", "createdAt": "2026-07-01 23:05:00" }
      ],
      "truths": {},
      "observationCount": 2
    }
  ]
}
```

### 2.3 CLI commands

* **Delete by ID**:
  ```bash
  asobi rm-obs my-entity 1 --id
  ```
* **Update by ID**:
  ```bash
  asobi update-obs my-entity 2 "next: code tests" --id
  ```

If `--id` is omitted, the CLI falls back to the legacy string-matching behavior to prevent breaking existing scripts.

---

## 3. Database Migration Path

To support seamless upgrades of existing databases without data loss or manual setup, `init_db()` runs an in-place migration check during database initialization:

1. Queries `PRAGMA table_info(asobi_observations)` to inspect the type of the `id` column.
2. If it is `TEXT` (the legacy UUID format), it triggers the migration:
   - Sets `PRAGMA foreign_keys = OFF` to prevent constraint violations.
   - Renames `asobi_observations` to `asobi_observations_old`.
   - Creates the new `asobi_observations` table using the `INTEGER PRIMARY KEY AUTOINCREMENT` schema.
   - Copies existing rows, ordering by `created_at` and the original `rowid` to preserve exact insertion sequence:
     ```sql
     INSERT INTO asobi_observations (entity_name, content, created_at)
     SELECT entity_name, content, created_at FROM asobi_observations_old ORDER BY created_at, rowid;
     ```
   - Drops the `asobi_observations_old` table.
   - Restores `PRAGMA foreign_keys = ON` and rebuilds the FTS5 virtual index.

---

## 4. Efficiency Evaluation

### 4.1 Token Efficiency
* **String-Match Deletion**: To delete an observation containing $N$ words, the CLI command args take $N$ tokens. For a typical 50-word observation, this is $\sim 65$ tokens (including command overhead).
* **ID-Match Deletion**: Deletion targets a simple integer (e.g. `1`). This takes exactly 1 token, representing a **$98\%$ token saving** for large updates or deletions.

### 4.2 Query and Execution Efficiency
* **Primary Key Indexing**: Finding, deleting, or updating a row by an integer primary key is a direct B-tree lookup ($O(\log N)$ or $O(1)$ in SQLite), avoiding string comparison operations on the `content` field.
* **Compact Sorting**: Sorting is done via `ORDER BY id` instead of string datetimes (`ORDER BY created_at, id`). Sorting on an integer primary key is highly optimized and index-backed in SQLite, bypassing text comparison overhead.
