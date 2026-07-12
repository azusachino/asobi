# Embedding model comparison — MiniLM-L6-v2 vs GTE-base-v1.5-Q

Baseline for the v0.5 document-tier model swap (see `CHANGELOG.md`). Recorded 2026-07-12 on the maintainer's macOS/arm64 machine. Numbers are indicative, not absolute — re-run the commands below to refresh on other hardware.

## Models

|  | all-MiniLM-L6-v2 (old) | gte-base-en-v1.5 int8-quant (current) |
| --- | --- | --- |
| fastembed variant | `AllMiniLML6V2` | `GTEBaseENV15Q` |
| dimension | 384 | 768 |
| params | ~22M | ~109M |
| max context | 256 tokens | 8192 tokens |
| on-disk model cache | ~96 MB | ~144 MB |

## Results

Corpus: this repo's `docs/` (11 Markdown files, ~1,851 lines). Warm model cache. `hyperfine --warmup 1`.

| Metric | MiniLM-L6-v2 (384d) | GTE-base-Q (768d) | Ratio |
| --- | --- | --- | --- |
| Ingest (full re-embed of corpus) | 4.31 s ± 0.01 | 10.52 s ± 0.15 | 2.44× slower |
| Query cold-start (load + embed 1 + search) | 180 ms ± 13 | 298 ms ± 7 | 1.65× slower |
| Model cache on disk | 96 MB | 144 MB | 1.5× larger |
| Vector storage per chunk | 384 × f32 | 768 × f32 | 2× larger |

Ingest time includes the one-time model load per process; query cold-start is the full per-invocation cost a CLI user feels (both models re-load per run since `asobi` is not a long-running server).

## Quality

Not numerically benchmarked here (no labelled relevance set). Verified qualitatively and in the gate test `recall::tests::test_recall_ranks_paraphrase_with_real_model`: keyword-free paraphrase queries rank the correct document first, which MiniLM's older training could not be relied on to do for technical prose. GTE-base was chosen specifically for its stronger performance on technical/programming text, which is asobi's primary content.

## Verdict

The regression is real but acceptable for a local knowledge-graph CLI: query cold-start stays well under a third of a second, ingest is an occasional operation, and disk grew only 1.5×. The int8-quantized build keeps the full model's 768-dim quality at ~1/3 the fp32 download (~144 MB vs ~530 MB). Quality on technical prose was judged worth the cost.

## Reproduce

```sh
# Build both binaries (old model from the pre-upgrade commit, current from HEAD).
CORPUS=docs
hyperfine --warmup 1 --runs 5 --prepare 'rm -f /tmp/bench/db.db*' \
  --command-name 'MiniLM-L6-v2 (384d)' \
  "env ASOBI_DATABASE_URL=/tmp/bench/db.db ASOBI_FASTEMBED_CACHE_DIR=<minilm-cache> <old-asobi> ingest $CORPUS" \
  --command-name 'GTE-base-v1.5-Q (768d)' \
  "env ASOBI_DATABASE_URL=/tmp/bench/db.db ASOBI_FASTEMBED_CACHE_DIR=<gte-cache> ./target/debug/asobi ingest $CORPUS"
```

> Note: inspect a running database only through the `asobi` CLI. The system `sqlite3` binary cannot read libSQL's WAL and will report stale/empty tables.
