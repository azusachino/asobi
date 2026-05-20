## Objective
Upgrade Rosemary to v0.2.0 — Multi-file Vector Knowledge Base.

## Status
DONE

## Completed Steps
- [x] Update dependencies (LanceDB, FastEmbed, text-splitter).
- [x] Implement EmbeddingProvider trait and FastEmbedProvider.
- [x] Add paragraph-level Chunker module.
- [x] Implement LanceDB VectorStore for semantic recall.
- [x] Upgrade libSQL schema (v2) with FTS5 and entity/relation support.
- [x] Build ingest pipeline for files and directories.
- [x] Implement hybrid recall (ANN + FTS5 re-ranking).
- [x] Implement digest command for LLM-assisted session recording.
- [x] Fix relate command with entity upsert safety.
- [x] Implement compact command for session pruning and cluster detection.
- [x] Wire all components into main.rs.

## Next Action
Start using the new vector-backed knowledge base for agent conversations.

## Last Updated
2026-03-03
