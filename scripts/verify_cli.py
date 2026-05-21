#!/usr/bin/env python3
"""Graph-only CLI integration checks for Rosemary.

This script intentionally exercises the built binary through subprocesses
instead of importing Rust internals. It is run by `make test-scripts` via `uv`.
"""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "rosemary"


def run(args: list[str], env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        [str(BIN), *args],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"command failed: rosemary {' '.join(args)}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    return result


def run_expect_failure(
    args: list[str], env: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        [str(BIN), *args],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode == 0:
        raise AssertionError(
            f"command unexpectedly succeeded: rosemary {' '.join(args)}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    return result


def graph(args: list[str], env: dict[str, str]) -> dict:
    return json.loads(run(args, env).stdout)


def entity_names(payload: dict) -> set[str]:
    return {entity["name"] for entity in payload["entities"]}


def observations(payload: dict, name: str) -> list[str]:
    for entity in payload["entities"]:
        if entity["name"] == name:
            return entity["observations"]
    return []


def main() -> None:
    subprocess.run(["cargo", "build"], cwd=ROOT, check=True)

    with tempfile.TemporaryDirectory(prefix="rosemary-cli-") as tmp:
        env = os.environ.copy()
        env["DATABASE_URL"] = str(Path(tmp) / "rosemary.db")

        run(["create-entities", "project-a", "project"], env)
        run(["create-entities", "project-a:session", "session"], env)
        run(["create-entities", "UserPreferences", "preference"], env)

        run(
            [
                "add-observations",
                "project-a",
                "Uses libSQL with FTS5 porter stemming for graph recall.",
            ],
            env,
        )
        run(
            [
                "add-observations",
                "project-a:session",
                "status: IN_PROGRESS; next: verify CLI handoff",
            ],
            env,
        )
        run(
            [
                "add-observations",
                "UserPreferences",
                "Prefer narrow graph commands over document-tier startup.",
            ],
            env,
        )
        run(["create-relations", "project-a", "UserPreferences", "follows"], env)

        opened = graph(["open-nodes", "project-a", "UserPreferences"], env)
        assert entity_names(opened) == {"project-a", "UserPreferences"}
        assert opened["relations"] == [
            {
                "from": "project-a",
                "to": "UserPreferences",
                "relationType": "follows",
            }
        ]

        stemmed = graph(["search-nodes", "stem"], env)
        assert "project-a" in entity_names(stemmed)

        for idx in range(5):
            run(["create-entities", f"limit-{idx}", "project"], env)
            run(["add-observations", f"limit-{idx}", "limitterm"], env)

        limited = graph(["search-nodes", "limitterm", "--limit", "3"], env)
        assert len(limited["entities"]) == 3

        name_fallback = graph(["search-nodes", "UserPreferences"], env)
        assert "UserPreferences" in entity_names(name_fallback)

        invalid_fts = graph(["search-nodes", "AND AND"], env)
        assert invalid_fts["entities"] == []

        suspicious_name = "cli-日本語-'; DROP TABLE mcp_entities; --"
        suspicious_observation = "quote:' newline:\n control:\x07 percent:%"
        run(["create-entities", suspicious_name, "project"], env)
        run(["add-observations", suspicious_name, suspicious_observation], env)
        suspicious = graph(["open-nodes", suspicious_name], env)
        assert entity_names(suspicious) == {suspicious_name}
        assert observations(suspicious, suspicious_name) == [suspicious_observation]

        injected = graph(["search-nodes", "'; DROP TABLE mcp_entities; --"], env)
        assert suspicious_name in entity_names(injected)
        still_there = graph(["read-graph"], env)
        assert {
            "project-a",
            "project-a:session",
            "UserPreferences",
            suspicious_name,
        }.issubset(entity_names(still_there))

        run(
            [
                "delete-observations",
                "project-a:session",
                "status: IN_PROGRESS; next: verify CLI handoff",
            ],
            env,
        )
        run(["add-observations", "project-a:session", "status: DONE"], env)

        session = graph(["open-nodes", "project-a:session"], env)
        assert observations(session, "project-a:session") == ["status: DONE"]

        run(["delete-entities", "UserPreferences"], env)
        after_delete = graph(["open-nodes", "project-a", "UserPreferences"], env)
        assert entity_names(after_delete) == {"project-a"}
        assert after_delete["relations"] == []

    with tempfile.TemporaryDirectory(prefix="rosemary-corrupt-") as tmp:
        db_path = Path(tmp) / "corrupt.db"
        db_path.write_bytes(b"not a sqlite database")
        env = os.environ.copy()
        env["DATABASE_URL"] = str(db_path)
        failed = run_expect_failure(["read-graph"], env)
        assert "database" in failed.stderr.lower()

    print("CLI graph integration checks passed")


if __name__ == "__main__":
    main()
