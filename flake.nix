{
  description = "asobi — persistent knowledge graph CLI for humans and LLM agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        # Edition 2024 needs a recent stable toolchain; rust-overlay tracks it.
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rustfmt"
            "clippy"
            "rust-src"
          ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            # Scripts + formatters
            pkgs.uv
            pkgs.ruff
            pkgs.bun
            # Task runner
            pkgs.gnumake
            # Native build dependencies for bundled SQLite
            pkgs.pkg-config
            pkgs.openssl
            pkgs.cmake
          ];

          env = {
            # Keep uv's managed interpreters inside the workspace cache.
            UV_PYTHON_PREFERENCE = "managed";
          };
        };
      }
    );
}
