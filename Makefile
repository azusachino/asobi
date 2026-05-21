NIX_RUN := $(if $(filter $(IN_NIX_SHELL),),nix develop --command ,)

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
	$(NIX_RUN) cargo build

build-documents:
	$(NIX_RUN) cargo build --features documents

run:
	$(NIX_RUN) cargo run -- $(ARGS)

test:
	$(NIX_RUN) cargo test -- --test-threads=1

test-documents:
	$(NIX_RUN) cargo test --features documents -- --test-threads=1

test-scripts:
	$(NIX_RUN) uv run scripts/verify_cli.py

fmt:
	$(NIX_RUN) cargo fmt
	$(NIX_RUN) prettier --write "**/*.{json,yaml,yml}" || true
	$(NIX_RUN) uv run ruff format . || true

lint:
	$(NIX_RUN) cargo clippy -- -D warnings
	$(NIX_RUN) uv run ruff check . || true

check: fmt lint test test-scripts

check-documents: build-documents test-documents

bench:
	$(NIX_RUN) cargo bench --bench graph

clean:
	$(NIX_RUN) cargo clean
	rm -rf target/

init:
	$(NIX_RUN) mise install || true
	$(NIX_RUN) uv venv --python 3.14 || true
	$(NIX_RUN) uv add ruff --dev || true
	mkdir -p scripts
