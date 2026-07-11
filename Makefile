.PHONY: help build build-documents run test test-documents test-scripts test-turso-scripts test-documents-scripts verify-storage-boundary verify-libsql verify-turso bench-libsql bench-turso fmt fmt-check lint check check-documents check-turso bench clean init

# Default task: Show help
help:
	@echo "Available tasks:"
	@echo "  build         - Compile the Asobi CLI and library"
	@echo "  build-documents - Compile with fastembed document commands"
	@echo "  run           - Run the Asobi CLI via cargo"
	@echo "  test          - Run all Rust tests"
	@echo "  test-documents - Run tests with the fastembed document feature"
	@echo "  test-scripts  - Run uv-managed graph-tier CLI integration checks"
	@echo "  test-turso-scripts - Run experimental Turso CLI integration checks"
	@echo "  test-documents-scripts - Run uv-managed document-tier CLI checks"
	@echo "  verify-storage-boundary - Reject provider references outside src/storage"
	@echo "  verify-libsql - Run the default libSQL CLI verification"
	@echo "  verify-turso - Run the experimental Turso CLI verification"
	@echo "  bench-libsql - Run graph benchmarks against the default build"
	@echo "  bench-turso - Build the Turso graph benchmark (set ASOBI_BACKEND=turso to select it)"
	@echo "  fmt           - Format Rust, Python, JSON, and YAML code"
	@echo "  fmt-check     - Verify formatting without writing (gate; run fmt to fix)"
	@echo "  lint          - Run Rust clippy and Python ruff"
	@echo "  check         - Run format check, lint, and tests (CI baseline, libSQL only)"
	@echo "  check-documents - Run document feature build and tests"
	@echo "  check-turso   - Build/test/verify the experimental Turso backend (opt-in)"
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

verify-storage-boundary:
	uv run --no-project python scripts/verify_storage_boundary.py

verify-libsql: build
	uv run scripts/verify_cli.py

verify-turso:
	cargo build --features turso-experimental
	uv run scripts/verify_cli.turso.py

bench-libsql:
	cargo bench --bench graph

# Turso is outside the default gate; libSQL is the supported default. To run
# this benchmark against Turso rather than the default backend, select it:
#   ASOBI_BACKEND=turso make bench-turso
bench-turso:
	cargo bench --features turso-experimental --bench graph

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
	cargo clippy --features documents -- -D warnings
	ruff check .

check: verify-storage-boundary fmt-check lint test test-scripts check-documents

check-documents: build-documents test-documents test-documents-scripts

# Experimental Turso backend is opt-in and not part of the default `check`
# baseline (which ships libSQL). Run this to exercise the feature-gated matrix.
check-turso:
	cargo clippy --features turso-experimental -- -D warnings
	cargo test --features turso-experimental -- --test-threads=1
	cargo build --features turso-experimental
	uv run scripts/verify_cli.turso.py

bench:
	cargo bench

clean:
	cargo clean
	rm -rf target/

init:
	@echo "Nix serves this repo. Enter the dev shell with:"
	@echo "  nix develop"
	@echo "(provides rust, uv, ruff, bun, and make)"
