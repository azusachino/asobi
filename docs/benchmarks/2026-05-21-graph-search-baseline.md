# Graph Search Baseline

Date: 2026-05-21

Command:

```bash
make bench
```

Dataset:

- entities: 10,000
- observations: 10,000
- selective query: `rareterm7777`, 11 hits
- broad query: `commonterm`, 10,000 hits
- open query: 3 named entities

Baseline before search limit, observation-load batching, and observation index:

| Case                   | Total          | Average       | Iterations |
| ---------------------- | -------------- | ------------- | ---------- |
| search_nodes selective | 184.781792 ms  | 3.695635 ms   | 50         |
| search_nodes broad     | 13.692743375 s | 2.738548675 s | 5          |
| open_nodes 3 names     | 817.826417 ms  | 817.826 us    | 1,000      |

The broad-search number represents returning all 10,000 full entities and
observations. That path behaves like a ranked graph export and should not be the
default search behavior.

## After Search Limit + Batched Loads

Changes:

- `search-nodes` defaults to top 100 results.
- `search-nodes --limit N` and MCP `search_nodes.limit` expose explicit larger result sets.
- Entity and observation loading is batched for `search_nodes` and `open_nodes`.
- `mcp_observations(entity_name)` is indexed.

Command:

```bash
make bench
```

Results:

| Case                        | Total       | Average    | Iterations | Hits   |
| --------------------------- | ----------- | ---------- | ---------- | ------ |
| search_nodes selective      | 51.713 ms  | 1.034 ms   | 50         | 11     |
| search_nodes broad capped   | 52.918 ms  | 10.584 ms  | 5          | 100    |
| search_nodes broad export   | 849.417 ms | 283.139 ms | 3          | 10,000 |
| open_nodes 3 names          | 62.863 ms  | 62.863 us  | 1,000      | 3      |

The default broad-search path improved from ~2.739 s to ~10.584 ms by returning
top-K results instead of exporting every matched entity. The explicit full export
path still returns all 10,000 entities, but batched loading cuts it to ~283 ms.
