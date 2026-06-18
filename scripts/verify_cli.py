#!/usr/bin/env python3
"""Graph-only CLI integration checks for Asobi.

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
BIN = ROOT / "target" / "debug" / "asobi"


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
            f"command failed: asobi {' '.join(args)}\n"
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
            f"command unexpectedly succeeded: asobi {' '.join(args)}\n"
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


def truths(payload: dict, name: str) -> dict[str, str]:
    for entity in payload["entities"]:
        if entity["name"] == name:
            return entity["truths"]
    return {}


def main() -> None:
    subprocess.run(["cargo", "build"], cwd=ROOT, check=True)

    with tempfile.TemporaryDirectory(prefix="asobi-cli-") as tmp:
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(Path(tmp) / "asobi.db")

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
        names = entity_names(opened)
        assert names == {"project-a", "UserPreferences"}
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
        # New normalization drops non-ascii and collapses separators
        normalized_suspicious_name = "cli-DROP-TABLE-mcp_entities"
        suspicious_observation = "quote:' newline:\n control:\x07 percent:%"
        run(["create-entities", suspicious_name, "project"], env)
        run(["add-observations", suspicious_name, suspicious_observation], env)
        suspicious = graph(["open-nodes", suspicious_name], env)
        assert entity_names(suspicious) == {normalized_suspicious_name}
        assert observations(suspicious, normalized_suspicious_name) == [
            suspicious_observation
        ]

        injected = graph(["search-nodes", "drop"], env)
        assert normalized_suspicious_name in entity_names(injected)
        still_there = graph(["read-graph"], env)
        assert {
            "project-a",
            "project-a:session",
            "UserPreferences",
            normalized_suspicious_name,
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

        # Truths: structured key-value attributes, upsert + delete
        run(["add-truth", "project-a", "language", "rust"], env)
        run(["add-truth", "project-a", "edition", "2021"], env)
        run(["add-truth", "project-a", "edition", "2024"], env)  # upsert replaces
        with_truths = graph(["open-nodes", "project-a"], env)
        assert truths(with_truths, "project-a") == {
            "language": "rust",
            "edition": "2024",
        }

        run(["delete-truth", "project-a", "language"], env)
        after_truth_delete = graph(["open-nodes", "project-a"], env)
        assert truths(after_truth_delete, "project-a") == {"edition": "2024"}

        run(["delete-entities", "UserPreferences"], env)
        after_delete = graph(["open-nodes", "project-a", "UserPreferences"], env)
        assert entity_names(after_delete) == {"project-a"}
        assert after_delete["relations"] == []

        # Stats test
        stats = run(["stats"], env).stdout
        assert "Knowledge Graph Statistics" in stats

        # Export test
        export_file = str(Path(tmp) / "backup.json")
        run(["export", "--output", export_file], env)
        assert Path(export_file).exists()

        # Reset test
        run(["reset", "--force"], env)
        empty_graph = graph(["read-graph"], env)
        assert empty_graph["entities"] == []
        assert empty_graph["relations"] == []

        # Import test
        run(["import", export_file], env)
        restored_graph = graph(["read-graph"], env)
        assert "project-a" in entity_names(restored_graph)

    with tempfile.TemporaryDirectory(prefix="asobi-corrupt-") as tmp:
        db_path = Path(tmp) / "corrupt.db"
        db_path.write_bytes(b"not a sqlite database")
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(db_path)
        failed = run_expect_failure(["read-graph"], env)
        assert "database" in failed.stderr.lower()

    batch_and_json_checks()
    skills_checks()

    print("CLI graph integration checks passed")


def batch_and_json_checks() -> None:
    """Coverage for batched writes and the global ``--json`` echo.

    ``create-entities`` takes repeated ``NAME TYPE`` pairs and
    ``create-relations`` takes repeated ``FROM TO TYPE`` triples in a single
    call (the underlying DB layer is already batch-capable). ``--json`` makes a
    mutation print the affected entities to stdout so a caller can confirm a
    write without a follow-up ``open-nodes``.
    """
    with tempfile.TemporaryDirectory(prefix="asobi-batch-") as tmp:
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(Path(tmp) / "asobi.db")

        # create-entities: one call, multiple NAME TYPE pairs.
        run(
            ["create-entities", "alpha", "task", "beta", "concept", "gamma", "ref"],
            env,
        )
        assert entity_names(graph(["read-graph"], env)) == {"alpha", "beta", "gamma"}

        # Argument count not a multiple of 2 is rejected with a clear message.
        bad_pairs = run_expect_failure(["create-entities", "x", "task", "y"], env)
        assert "pair" in bad_pairs.stderr.lower()

        # create-relations: one call, multiple FROM TO TYPE triples.
        run(
            ["create-relations", "alpha", "beta", "uses", "alpha", "gamma", "blocks"],
            env,
        )
        rels = graph(["read-graph"], env)["relations"]
        assert {(r["from"], r["to"], r["relationType"]) for r in rels} == {
            ("alpha", "beta", "uses"),
            ("alpha", "gamma", "blocks"),
        }

        # Argument count not a multiple of 3 is rejected.
        bad_triples = run_expect_failure(
            ["create-relations", "a", "b", "uses", "c"], env
        )
        assert "triple" in bad_triples.stderr.lower()

        # --json: create-entities echoes the created entity to stdout.
        echoed = json.loads(
            run(["create-entities", "delta", "task", "--json"], env).stdout
        )
        assert "delta" in entity_names(echoed)

        # --json: add-observations returns the affected entity with its trail.
        obs_echo = json.loads(
            run(["add-observations", "delta", "first obs", "--json"], env).stdout
        )
        assert observations(obs_echo, "delta") == ["first obs"]

        # --json: create-relations shows the relation among its endpoints.
        rel_echo = json.loads(
            run(["create-relations", "delta", "alpha", "uses", "--json"], env).stdout
        )
        assert {
            "from": "delta",
            "to": "alpha",
            "relationType": "uses",
        } in rel_echo["relations"]

        # --json: add-truth / delete-truth return the entity's current truths.
        truth_echo = json.loads(
            run(["add-truth", "delta", "status", "READY", "--json"], env).stdout
        )
        assert truths(truth_echo, "delta") == {"status": "READY"}

        # --json: delete-entities reports the removed names (entities are gone,
        # so there is nothing to open — the shape is a deletion receipt).
        del_echo = json.loads(run(["delete-entities", "gamma", "--json"], env).stdout)
        assert del_echo == {"deleted": ["gamma"]}


def skills_checks() -> None:
    """End-to-end coverage for the `skills` command group, including the
    git edge cases (missing git binary, unreachable remote, bad local path).

    `ASOBI_HOME` is set alongside `ASOBI_DATABASE_URL` so the skills
    cache (`paths.caches_dir()`) stays inside the temp dir — it resolves from
    HOME/XDG, not from the database URL — and never touches global state.
    """
    with tempfile.TemporaryDirectory(prefix="asobi-skills-") as tmp:
        root = Path(tmp)
        env = os.environ.copy()
        env["ASOBI_HOME"] = str(root / "home")
        env["ASOBI_DATABASE_URL"] = str(root / "asobi.db")

        # Local skill source dir: one full skill + one name-fallback skill.
        src = root / "src-skills"
        (src / "nested").mkdir(parents=True)
        (src / "alpha.md").write_text(
            "---\nname: alpha\ndescription: Alpha skill\n---\nAlpha body here\n"
        )
        # Only a description: name falls back to the parent dir ("nested").
        (src / "nested" / "SKILL.md").write_text(
            "---\ndescription: Nested skill\n---\nNested body\n"
        )

        # Install (local dir => version "local", no git needed).
        run(["skills", "install", str(src), "--all"], env)

        listed = run(["skills"], env).stdout
        assert "Installed Skills:" in listed
        assert "alpha" in listed
        assert "nested" in listed
        assert "Alpha skill" in listed

        # show resolves a short name and prints the raw body unescaped.
        shown = run(["skills", "show", "alpha"], env).stdout
        assert "Alpha body here" in shown

        # Remove by source string clears every skill from that source.
        run(["skills", "remove", str(src)], env)
        assert "No skills installed." in run(["skills"], env).stdout

        # --select installs only the named skill, not the rest.
        run(["skills", "install", str(src), "--select", "alpha"], env)
        selected = run(["skills"], env).stdout
        assert "alpha" in selected
        assert "nested" not in selected
        run(["skills", "remove", str(src)], env)

        # --select with an unknown name fails.
        bad_select = run_expect_failure(
            ["skills", "install", str(src), "--select", "ghost"], env
        )
        assert "not found" in bad_select.stderr.lower()

        # Edge case: local path that does not exist.
        missing = run_expect_failure(
            ["skills", "install", str(root / "does-not-exist"), "--all"], env
        )
        assert "does not exist" in missing.stderr.lower()

        # Edge case: remote (git URL) unreachable — git present, clone fails.
        # file:// avoids any network so the check stays offline and fast.
        unreachable = run_expect_failure(
            ["skills", "install", f"file://{root}/no-such-repo.git", "--all"], env
        )
        assert "clone" in unreachable.stderr.lower()

        # Edge case: git binary not installed — strip git from PATH and point
        # at a git URL so resolution reaches the remote path.
        no_git_env = dict(env)
        no_git_env["PATH"] = str(root / "empty-bin")
        (root / "empty-bin").mkdir()
        no_git = run_expect_failure(
            ["skills", "install", "https://example.com/owner/repo.git", "--all"],
            no_git_env,
        )
        assert "git" in no_git.stderr.lower()


if __name__ == "__main__":
    main()
