#!/usr/bin/env python3
"""Reject provider-shaped references outside the storage boundary.

The application layer may depend on ``api::v2`` capabilities and on the
``storage::Storage`` composite, but it must not name a concrete provider,
driver type, SQL module, or provider-owned state path. Backend-specific tests
must opt in explicitly with ``// storage-boundary: provider-test``.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PATHS = (ROOT / "src", ROOT / "tests", ROOT / "benches")
PROVIDER_RE = re.compile(r"\b(?:turso|libsql|sqlx|TursoStore|LibsqlStore)\b", re.I)
ALLOW_MARKER = "storage-boundary: provider-test"


def rust_files(paths: tuple[Path, ...]):
    for base in paths:
        if base.is_file() and base.suffix == ".rs":
            yield base
        elif base.is_dir():
            yield from sorted(base.rglob("*.rs"))


def source_and_tests(path: Path) -> tuple[str, bool]:
    text = path.read_text()
    if ALLOW_MARKER in text:
        marker = re.search(r"#\[cfg\((?:all\()?test", text)
        if marker:
            return text[: marker.start()], True
    return text, False


def allowed(path: Path) -> bool:
    relative = path.relative_to(ROOT)
    provider_test = relative.parts[0] in {"tests", "benches"}
    return relative.parts[:2] == ("src", "storage") or (
        provider_test and ALLOW_MARKER in path.read_text()
    )


def violations(paths: tuple[Path, ...]) -> list[str]:
    errors: list[str] = []
    for path in rust_files(paths):
        text, test_fixture = source_and_tests(path)
        relative = path.relative_to(ROOT)
        if relative.parts[:2] == ("src", "storage"):
            continue
        if allowed(path) and (
            test_fixture or relative.parts[0] in {"tests", "benches"}
        ):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if PROVIDER_RE.search(line):
                errors.append(f"{path.relative_to(ROOT)}:{number}: {line.strip()}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "paths", nargs="*", type=Path, help="Rust files or directories to scan"
    )
    args = parser.parse_args()
    paths = (
        tuple((ROOT / path if not path.is_absolute() else path) for path in args.paths)
        or DEFAULT_PATHS
    )
    errors = violations(paths)
    if errors:
        print("storage boundary violations:", file=sys.stderr)
        print("\n".join(errors), file=sys.stderr)
        print(
            "Mark an intentionally backend-specific test with "
            f"// {ALLOW_MARKER}, or move provider code under src/storage/.",
            file=sys.stderr,
        )
        return 1
    print("storage boundary: clean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
