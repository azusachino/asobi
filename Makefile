NIX_RUN := $(if $(filter $(IN_NIX_SHELL),),nix develop --command ,)

.PHONY: help build run test fmt lint check clean run-examples init

# Default task: Show help
help:
	@echo "Available tasks:"
	@echo "  build         - Compile the Rosemary CLI and library"
	@echo "  run           - Run the Rosemary CLI via cargo"
	@echo "  test          - Run all Rust tests"
	@echo "  fmt           - Format Rust, Python, Markdown, and TOML code"
	@echo "  lint          - Run Rust clippy, Python ruff, and Markdown lint"
	@echo "  check         - Run format check, lint, and tests (CI baseline)"
	@echo "  run-examples  - Run a specific async example (EXAMPLE=name)"
	@echo "  clean         - Remove build artifacts"
	@echo "  init          - Initialize development environment (mise, uv, etc.)"

build:
	$(NIX_RUN) cargo build

run:
	$(NIX_RUN) cargo run -- $(ARGS)

test:
	$(NIX_RUN) cargo test

fmt:
	$(NIX_RUN) cargo fmt
	$(NIX_RUN) taplo fmt
	$(NIX_RUN) prettier --write "**/*.{md,json,yaml,yml}"
	$(NIX_RUN) uv run ruff format . || true

lint:
	$(NIX_RUN) cargo clippy -- -D warnings
	$(NIX_RUN) uv run ruff check . || true
	$(NIX_RUN) find . -name "*.md" -not -path "./target/*" -not -path "./.venv/*" -exec uv run pymarkdown -c .pymarkdown.json scan {} + || true

check: fmt lint test

run-examples:
	$(NIX_RUN) cargo run --example $(EXAMPLE)

clean:
	$(NIX_RUN) cargo clean
	rm -rf target/

init:
	$(NIX_RUN) mise install || true
	$(NIX_RUN) uv venv --python 3.14 || true
	$(NIX_RUN) uv add ruff mdformat-gfm pymarkdownlnt --dev || true
	mkdir -p scripts kb/topics
