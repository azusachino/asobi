# OSS agent-memory lifecycle survey

Date: 2026-07-12. Purpose: decide which entity/task/knowledge lifecycle mechanisms
asobi should adopt, by surveying open-source agent-memory systems. This is a durable
reference — read it instead of re-running the web search.

Asobi's model for context: append-only **observations** (capped at 200/entity),
key -> value **truths** (upsert = current state), directed **relations**, plus
semantic recall (fastembed vectors + FTS5) over ingested markdown.

Lifecycle gaps under review: decay/forgetting, dedup/merge, episodic -> semantic
promotion, temporal/bitemporal validity, conflict resolution, summarization/archival.

## Project survey

### mem0 (Apache-2.0, ~60k stars, very active)
Flat vector store of atomic LLM-extracted facts. Signature mechanism:
**LLM-driven ADD/UPDATE/DELETE/NOOP** per candidate fact against top-k similar
existing memories — one decision engine covering dedup + conflict resolution.
v3 rewrite shifted to ADD-only, rank-by-recency-at-read-time (keeps contradictory
facts side by side, no invalidation). No native decay (OSS gap, issue #5330;
decay is Platform-only). No archival tier (DELETE = gone). Graph mode removed from
OSS v3.
- https://github.com/mem0ai/mem0
- https://arxiv.org/html/2504.19413v1
- https://github.com/mem0ai/mem0/issues/5330

### Letta / MemGPT (Apache-2.0, ~23k stars, active, pivoting)
OS-inspired tiers: **core** (in-context editable blocks) / **recall** (full message
history) / **archival** (vector cold store). Promotion via **sleep-time agents** — a
background agent reflects on history and distills durable facts. Recursive
summarization for message eviction. No decay, no bitemporal (known gap vs Zep);
conflict resolution = agent manually overwriting a block string, no provenance kept.
Dedup requested but unbuilt (issue #3116).
- https://github.com/letta-ai/letta
- https://www.letta.com/blog/sleep-time-compute/
- https://docs.letta.com/letta-agent/memory

### Zep / Graphiti (Apache-2.0, ~28.6k stars, very active) — temporal-validity reference
Temporal property graph: **Episodic** (immutable raw, provenance root) ->
**Entity/fact** subgraph -> **Community** clusters. Headline: **bi-temporal edges**
with four timestamps — `t_valid`/`t_invalid` (valid time) and `created_at`/`expired_at`
(transaction time). Conflict resolution = **invalidate, don't delete**: a
contradicting episode sets the old edge's `t_invalid` to the new edge's `t_valid`,
keeping it for historical queries. **Dedup is heuristics-first** (Jaccard over shingles
>= 0.9 accept, no LLM; edges via RRF-fused retrieval, LLM only on the reduced candidate
set) — deliberately cheap. Compaction via incremental label-propagation communities +
summaries. No scored decay (keep-forever + invalidation).
- https://arxiv.org/abs/2501.13956
- https://github.com/getzep/graphiti
- https://deepwiki.com/getzep/graphiti

### cognee (Apache-2.0, ~27.6k stars, very active)
Everything is a recursive `DataPoint` across graph+vector+relational stores.
**Content-hash dedup at ingest** + **ontology-URI entity merge**. `forget(dataset=...)`
deletes tagged records precisely (no decay). **Memify** = continuous usage-driven
post-processing (prune stale nodes, reweight frequently-used edges). Native temporal =
valid-time-only events; true bitemporal only by delegating to Graphiti-core.
- https://github.com/topoteretes/cognee
- https://docs.cognee.ai/guides/time-awareness

### basic-memory (AGPL-3.0, ~3.4k stars, active) — structurally closest to asobi
Markdown files = entities; categorized bullet **observations**; wikilink **relations**.
Near-identical vocabulary to asobi but **no key->value truth layer**. Lifecycle lives in
optional *skills* (LLM workflows, not core code): `memory-defrag` (weekly cron: split
bloated files, merge duplicates, prune stale refs, with a reviewable plan +
"(review needed)" tags), `memory-reflect` (sleep-time-style episodic -> semantic
consolidation), `memory-lifecycle` (archive-never-delete via folder moves). Raw daily
notes never modified = audit trail.
- https://github.com/basicmachines-co/basic-memory
- https://github.com/basicmachines-co/basic-memory-skills

### LangMem (MIT, ~1.5k stars, high-usage, pre-1.0)
Namespaced KV store; explicit **semantic / episodic / procedural** split. **Memory
Manager** does insert/update/delete reconciliation, hot-path (tool mid-conversation) or
background. **Profiles** = single structured JSON patched in place (truth-like);
Collections = append docs. Retrieval blends similarity + importance + strength (formula
unpublished). No bitemporal, no documented TTL.
- https://github.com/langchain-ai/langmem
- https://langchain-ai.github.io/langmem/concepts/conceptual_guide/

### Memary (MIT, ~2.6k stars, slowing)
Neo4j/FalkorDB graph + Entity Knowledge Store with **frequency + recency** per entity.
"Forgetting" = top-N by frequency x recency surfaced into context (implicit
non-retrieval, not deletion/decay). Set-based dedup. No conflict resolution. Single
last-seen timestamp, no versioning.
- https://github.com/kingjulio8238/Memary
- https://kingjulio8238.github.io/memarydocs/concepts/

### Cipher / ByteRover CLI (Elastic-2.0, ~4.9k stars, active)
Coding-agent memory via MCP; cache+db+vector+KG layers. Context tree with git-like
branch/commit/merge + `brv curate` (human promotion gate). Cap-based retention
(`maxEntries` default 1000). Dedup/decay/conflict resolution undocumented publicly.
- https://github.com/campfirein/cipher
- https://docs.byterover.dev/cipher/memory-overview

### txtai (Apache-2.0, ~12.7k stars, very active)
Embeddings database with a derived semantic graph (PageRank/centrality/traversal). Pure
retrieval framework — no lifecycle features (no decay, dedup, temporal, conflict). Any
of it must be built on top.
- https://github.com/neuml/txtai

### Zettelkasten tools (Foam/Logseq/Obsidian/Dendron) — process, not code
Transferable idea: fleeting -> literature -> permanent/evergreen note promotion as a
deliberate editorial act; atomicity ("one note, one claim") makes duplicates visually
obvious; bidirectional backlinks are the maintenance structure. No automated
decay/conflict — the human is the compaction engine.
- https://www.atlasworkspace.ai/blog/zettelkasten-method-guide

## Cross-cutting findings

1. **Nobody ships scored/probabilistic decay in OSS core.** mem0, Zep, cognee, Letta all
   prefer explicit invalidation or deletion. Memary's ranking is retrieval-time
   surfacing, not forgetting. Ebbinghaus-curve decay is a consistent wishlist item, not
   shipped. This validates avoiding auto-decay.
2. **Bitemporal validity is the one genuine differentiator, and only Graphiti truly ships
   it.** cognee borrows it; everyone else lacks it. asobi's truths carry current state
   but no valid/transaction time separation.
3. **Dedup + conflict resolution are usually one operation** — either LLM-driven
   (mem0/LangMem: ADD/UPDATE/DELETE/NOOP) or heuristics-first (Graphiti: Jaccard/RRF, LLM
   last). Graphiti's cheap-heuristics-first approach fits a simplicity-valuing CLI far
   better than LLM-per-write.
4. **Promotion is done in the background (sleep-time compute)** across Letta, LangMem,
   basic-memory — a scheduled/idle reflection pass, not inline. basic-memory implements
   it as an on-demand skill, not engine code — a cheap model to copy.
5. **asobi's model is structurally near-unique**: append-only observations (episodic) +
   key->value truths (semantic/current) + directed relations. Nobody else has the
   observation/truth split as a schema guarantee. The truth-upsert is already a natural
   conflict-resolution point; the untapped questions are what triggers observation ->
   truth promotion and whether overwriting a truth preserves the loser's provenance.

## Recommendations for asobi, ranked by value/effort

### Rank 1 — Provenance-preserving truth overwrite (highest value, lowest effort)
Today a truth upsert silently discards the previous value. Adopt Graphiti's
"invalidate, don't delete" cheaply: on `truth` overwrite, auto-emit an observation
capturing the old value + timestamp (e.g. "status: in-progress -> done"). Zero new
storage primitives — reuses the append-only observation trail as the audit log. Best fit
for asobi's model; closes conflict-resolution + partial-temporal gaps at once. Maps to
asobi's stated status-as-truth philosophy — just make the transition record automatic.

### Rank 2 — Lightweight valid-time on truths (high value, low-medium effort)
Add `valid_from` (and optional `valid_until`) timestamps to truths — valid-time only,
not full bitemporal. Enables "what was true when" queries and lets `search --where`
filter by as-of date. Graphiti's most-cited feature in a form proportionate to a local
CLI. Stop short of dual transaction-time columns (see non-goals).

### Rank 3 — Heuristic dedup/merge as an explicit maintenance command (medium/medium)
Extend the existing `compact`/`digest` path with a cheap, deterministic near-duplicate
detector for entities/observations: Jaccard/shingle or FTS5 similarity (asobi already has
FTS5 + vectors) to *propose* merges, gated behind human confirmation — basic-memory's
`memory-defrag` "reviewable plan + (review needed)" pattern and Graphiti's
heuristics-first stance. Do NOT do LLM-per-write dedup.

### Rank 4 — Background promotion as a skill, not engine code (medium value, low effort)
asobi already has episodic observations and semantic truths. Add a reflection
skill/command (basic-memory `memory-reflect` / Letta sleep-time model) that reviews
recent observations and proposes promoting recurring signals into truths or new
relations. Keep it out of the write hot-path; make it an explicit invocation. Low effort
because it's prompt/policy, not schema.

### Rank 5 — Archive-never-delete lifecycle state (lower value, low effort)
The 200-observation cap currently forces loss. Adopt basic-memory's archive-by-status
instead of hard eviction: an `archived` truth (or status value) hides entities/
observations from default queries while preserving them, surfaced via a flag. Reuses
status-as-truth; no new tier needed.

### Non-goals (over-engineering to avoid — asobi values simplicity)
- **Scored/Ebbinghaus decay and recency-weighted ranking** — no OSS peer ships it; adds
  tuning burden and silent surprise. Explicit archival (Rank 5) covers the real need.
- **Full bi-temporal (dual valid-time x transaction-time columns)** — Graphiti-grade
  complexity; valid-time alone (Rank 2) captures ~90% of the value for a local CLI.
- **LLM-driven per-write ADD/UPDATE/DELETE/NOOP** (mem0/LangMem) — adds latency, cost,
  nondeterminism to every write; truth-upsert already resolves conflicts
  deterministically.
- **Community detection / label-propagation clustering** (Graphiti/txtai) —
  retrieval-scale machinery unjustified for a personal/agent graph.
- **Git-like branch/merge memory versioning** (Cipher MemFS) — heavyweight; the Rank-1
  observation trail gives lineage without a VCS layer.

## Takeaway

Asobi's observation/truth split is already the scaffolding every competitor lacks. The
highest-leverage moves are to auto-record truth transitions as observations (Rank 1) and
timestamp truth validity (Rank 2), both nearly free given the existing schema, while
treating decay and full bitemporality as deliberate non-goals.
