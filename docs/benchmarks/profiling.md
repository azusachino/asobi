# Performance profiling

Use the scaling benchmark for size trends and Criterion for statistically stable hot-path comparisons.

```bash
make bench-graph
make bench-criterion
make bench-alloc
make bench-sql-plans
make bench-tasks
make bench-storage
```

Criterion writes reports under `target/criterion/`. DHAT writes `dhat-heap.json`; open it with the DHAT viewer linked in the file. Run the same command before and after a change and compare medians, confidence intervals, throughput, total allocated bytes, and allocation sites.

The harnesses cover graph search, entity reads, statistics, SQLite FTS5, task claims, allocation-heavy graph reads, and query plans for FTS, truth filters, relation expansion, and name fallback.
