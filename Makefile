.PHONY: help build build-documents run test test-documents test-scripts fmt lint check check-documents bench clean init

# Default task: Show help
help:
	@echo "Available tasks:"
	@echo "  build         - Compile the Rosemary CLI and library"
	@echo "  build-documents - Compile with LanceDB/fastembed document commands"
	@echo "  run           - Run the Rosemary CLI via cargo"
	@echo "  test          - Run all Rust tests"
	@echo "  test-documents - Run tests with LanceDB/fastembed document feature"
	@echo "  test-scripts  - Run uv-managed CLI integration checks"
	@echo "  fmt           - Format Rust, Python, JSON, and YAML code"
	@echo "  lint          - Run Rust clippy and Python ruff"
	@echo "  check         - Run format check, lint, and tests (CI baseline)"
	@echo "  check-documents - Run document feature build and tests"
	@echo "  bench         - Run graph-tier benchmark harness"
	@echo "  clean         - Remove build artifacts"
	@echo "  init          - Initialize development environment (mise, uv, etc.)"

build:
	cargo build

build-documents:
	cargo build --features documents

run:
	cargo run -- $(ARGS)

test:
	cargo test -- --test-threads=1

test-documents:
	cargo test --features documents -- --test-threads=1

test-scripts:
	uv run scripts/verify_cli.py

fmt:
	cargo fmt
	bun x prettier --write "**/*.{json,yaml,yml}" || true
	ruff format . || true

lint:
	cargo clippy -- -D warnings
	ruff check . || true

check: fmt lint test test-scripts

check-documents: build-documents test-documents

bench:
	cargo bench --bench graph

clean:
	cargo clean
	rm -rf target/

init:
	mise install
	uv venv --python 3.14
	uv add ruff --dev
	mkdir -p scripts
