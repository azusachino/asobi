# Performance profiling

Use the scaling benchmark for size trends and Criterion for statistically stable hot-path comparisons.

```bash
ASOBI_BENCH_SIZES=1000,10000 make bench-libsql
ASOBI_BENCH_SIZE=10000 make bench-criterion
ASOBI_VECTOR_BENCH_SIZE=10000 make bench-vector-criterion
ASOBI_BENCH_SIZE=10000 make bench-alloc
make bench-sql-plans
```

Criterion writes reports under `target/criterion/`. DHAT writes `dhat-heap.json`; open it with the DHAT viewer linked in the file. Run the same command before and after a change and compare medians, confidence intervals, throughput, total allocated bytes, and allocation sites.

The harnesses cover selective and broad graph search, broad-result hydration, entity reads, statistics, vector top-k search, reset, allocation-heavy graph reads, and query plans for FTS, truth filters, relation expansion, and name fallback.
