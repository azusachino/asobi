.PHONY: help build build-documents run test test-documents test-scripts test-turso-scripts test-documents-scripts fmt fmt-check lint check check-documents bench clean init

# Default task: Show help
help:
	@echo "Available tasks:"
	@echo "  build         - Compile the Asobi CLI and library"
	@echo "  build-documents - Compile with Turso/fastembed document commands"
	@echo "  run           - Run the Asobi CLI via cargo"
	@echo "  test          - Run all Rust tests"
	@echo "  test-documents - Run tests with Turso/fastembed document feature"
	@echo "  test-scripts  - Run uv-managed graph-tier CLI integration checks"
	@echo "  test-turso-scripts - Run the Turso API/CLI integration checks"
	@echo "  test-documents-scripts - Run uv-managed document-tier CLI checks"
	@echo "  fmt           - Format Rust, Python, JSON, and YAML code"
	@echo "  fmt-check     - Verify formatting without writing (gate; run fmt to fix)"
	@echo "  lint          - Run Rust clippy and Python ruff"
	@echo "  check         - Run format check, lint, and tests (CI baseline)"
	@echo "  check-documents - Run document feature build and tests"
	@echo "  bench         - Run graph-tier benchmark harness"
	@echo "  clean         - Remove build artifacts"
	@echo "  init          - Print dev environment setup (nix develop)"

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

test-turso-scripts:
	uv run scripts/verify_cli.turso.py

test-documents-scripts:
	uv run scripts/verify_documents_cli.py

fmt:
	cargo fmt
	bun x prettier --write "**/*.{json,yaml,yml}"
	ruff format .

fmt-check:
	cargo fmt --check
	bun x prettier --check "**/*.{json,yaml,yml}"
	ruff format --check .

lint:
	cargo clippy -- -D warnings
	ruff check .

check: fmt-check lint test test-scripts test-turso-scripts check-documents

check-documents: build-documents test-documents test-documents-scripts

bench:
	cargo bench

clean:
	cargo clean
	rm -rf target/

init:
	@echo "Nix serves this repo. Enter the dev shell with:"
	@echo "  nix develop"
	@echo "(provides rust, uv, ruff, bun, and make)"
