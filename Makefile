.PHONY: help build run test test-scripts verify-storage-boundary bench bench-compile bench-graph bench-criterion bench-alloc bench-sql-plans bench-tasks bench-storage fmt fmt-check lint check clean init

help:
	@echo "Available tasks:"
	@echo "  build                 Compile the Asobi CLI and library"
	@echo "  run                   Run the Asobi CLI via cargo"
	@echo "  test                  Run all Rust tests serially"
	@echo "  test-scripts          Run built-CLI integration checks"
	@echo "  verify-storage-boundary  Check provider encapsulation"
	@echo "  bench                 Run all benchmark harnesses"
	@echo "  bench-compile         Compile all benchmark targets without running them"
	@echo "  bench-graph           Run graph benchmarks"
	@echo "  bench-criterion       Run graph Criterion benchmarks"
	@echo "  bench-alloc           Write a DHAT allocation profile"
	@echo "  bench-sql-plans       Print SQLite query plans"
	@echo "  bench-tasks           Benchmark task dispatch"
	@echo "  bench-storage         Benchmark SQLite storage hot paths"
	@echo "  fmt / fmt-check       Format or verify Rust, Python, JSON, YAML, and Markdown"
	@echo "  lint                  Run Rust clippy and Python ruff"
	@echo "  check                 Run the complete local quality gate"
	@echo "  clean                 Remove build artifacts"

build:
	cargo build

run:
	cargo run -- $(ARGS)

test:
	cargo test -- --test-threads=1

test-scripts: build
	uv run --with fastjsonschema scripts/verify_cli.py
	uv run --no-project python scripts/use_cases.py

verify-storage-boundary:
	uv run --no-project python scripts/verify_storage_boundary.py

bench:
	cargo bench

bench-compile:
	cargo bench --no-run

bench-graph:
	cargo bench --bench graph

bench-criterion:
	cargo bench --bench graph_criterion

bench-alloc:
	cargo bench --bench allocations

bench-sql-plans:
	cargo bench --bench sql_plans

bench-tasks:
	cargo bench --bench tasks

bench-storage:
	cargo bench --bench storage

fmt:
	cargo fmt
	bun x prettier --write "**/*.{json,yaml,yml,md}"
	ruff format .

fmt-check:
	cargo fmt --check
	bun x prettier --check "**/*.{json,yaml,yml,md}"
	ruff format --check .

lint:
	cargo clippy -- -D warnings
	ruff check .

check: verify-storage-boundary fmt-check lint test test-scripts bench-compile

clean:
	cargo clean

init:
	@echo "Nix serves this repo. Enter the dev shell with:"
	@echo "  nix develop"
