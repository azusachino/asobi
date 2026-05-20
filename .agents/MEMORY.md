# Project Memory

## 2026-03-03 — Hybrid Storage Architecture

**Fact/Decision:** Chose a three-tier storage strategy: Markdown for durability, libSQL for structured relations, and LanceDB for semantic vector recall.
**Why it matters:** Balances human readability with high-performance AI retrieval and structured knowledge mapping.

## 2026-03-03 — Dependency Trimming

**Fact/Decision:** Removed async playground dependencies (flume, async-task, reqwest) and kept only core KB requirements.
**Why it matters:** Keeps the binary small, focused, and reduces maintenance overhead for the knowledge base.

## 2026-03-03 — Unified Database Refactor

**Decision:** Consolidated libSQL and LanceDB into a single libSQL database.
*(Note: Later superseded by v0.2.0 hybrid model)*

## 2026-03-03 — Project Infrastructure (mise/make/python)

**Decision:** Adopted mise for tool management, Makefile for tasks, and uv for Python 3.14 environment.
**Reason:** Aligns with user's preferred workflow and enables part of the main application to use Python scripts.

## 2026-03-03 — Hybrid Recall Re-ranking (v0.2.0)

**Decision:** Implemented a hybrid recall strategy: `vector_score * 0.7 + fts5_bm25 * 0.3`.
**Reason:** Combines semantic depth of vectors with high-precision keyword matching of FTS5.

## 2026-03-03 — Standard vs. Contentless FTS5

**Decision:** Switched from contentless FTS5 to standard FTS5 storing its own data.
**Reason:** Avoids the inability to `DELETE` specific rows by `rowid` in contentless tables, which is needed for clean `upsert_topic` implementation.
