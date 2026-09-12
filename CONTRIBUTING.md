# Contributing to Asobi

Thank you for your interest in improving Asobi!

## Standards

- **Conventional Commits**: We use [Conventional Commits](https://www.conventionalcommits.org/). Please use prefixes like `feat:`, `fix:`, `chore:`, `docs:`, or `deploy:`.
- **No Emojis**: Please refrain from using emojis in commit messages.
- **Verification**: Before submitting a PR, ensure all checks pass locally by running:

  ```bash
  make check
  ```

## Development Workflow

1. Fork the repository.
2. Run `make init` (`mise install`) to get the pinned Rust, uv, bun, and ruff.
3. Create a new branch for your feature or fix.
4. Run `make check` to ensure your changes adhere to project standards.
5. Submit a PR.

We prioritize simple, idiomatic solutions over clever or speculative abstractions.
