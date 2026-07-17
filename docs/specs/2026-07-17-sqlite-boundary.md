# v0.6.0 SQLite product boundary

Asobi 0.6.0 keeps the local graph workflow on bundled SQLite through rusqlite. The product contract is graph state, FTS5 search, skills, task dispatch, backup/restore, and the one-way `compact` projection from graph state to Markdown topics.

The core product deliberately has no document ingestion, semantic embedding, vector index, or alternate backend feature. Those surfaces are not required for the daily agent workflow and are not part of the release build or verification matrix.

SQLite is configured with WAL, foreign keys, bounded busy timeouts, and immediate write transactions. FTS5 is an SQLite acceleration detail exposed as the truthful `keyword_search_kind` capability. Logical JSON export/import is portable; physical backup/restore is a local SQLite operation.
