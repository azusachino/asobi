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

        run(["new", "project-a", "project"], env)
        run(["new", "project-a:session", "session"], env)
        run(["new", "UserPreferences", "preference"], env)

        run(
            [
                "obs",
                "project-a",
                "Uses Turso native FTS for graph recall.",
            ],
            env,
        )
        run(
            [
                "obs",
                "project-a:session",
                "status: IN_PROGRESS; next: verify CLI handoff",
            ],
            env,
        )
        run(
            [
                "obs",
                "UserPreferences",
                "Prefer narrow graph commands over document-tier startup.",
            ],
            env,
        )
        run(["link", "project-a", "UserPreferences", "follows"], env)

        opened = graph(["show", "project-a", "UserPreferences"], env)
        names = entity_names(opened)
        assert names == {"project-a", "UserPreferences"}
        assert opened["relations"] == [
            {
                "from": "project-a",
                "to": "UserPreferences",
                "relationType": "follows",
            }
        ]

        keyword_match = graph(["search", "Turso"], env)
        assert "project-a" in entity_names(keyword_match)

        for idx in range(5):
            run(["new", f"limit-{idx}", "project"], env)
            run(["obs", f"limit-{idx}", "limitterm"], env)

        limited = graph(["search", "limitterm", "--limit", "3"], env)
        assert len(limited["entities"]) == 3

        name_fallback = graph(["search", "UserPreferences"], env)
        assert "UserPreferences" in entity_names(name_fallback)

        invalid_fts = graph(["search", "AND AND"], env)
        assert invalid_fts["entities"] == []

        suspicious_name = "cli-日本語-'; DROP TABLE mcp_entities; --"
        # New normalization drops non-ascii and collapses separators
        normalized_suspicious_name = "cli-DROP-TABLE-mcp_entities"
        suspicious_observation = "quote:' newline:\n control:\x07 percent:%"
        run(["new", suspicious_name, "project"], env)
        run(["obs", suspicious_name, suspicious_observation], env)
        suspicious = graph(["show", suspicious_name], env)
        assert entity_names(suspicious) == {normalized_suspicious_name}
        assert observations(suspicious, normalized_suspicious_name) == [
            suspicious_observation
        ]

        injected = graph(["search", "drop"], env)
        assert normalized_suspicious_name in entity_names(injected)
        still_there = graph(["graph"], env)
        assert {
            "project-a",
            "project-a:session",
            "UserPreferences",
            normalized_suspicious_name,
        }.issubset(entity_names(still_there))

        run(
            [
                "rm-obs",
                "project-a:session",
                "status: IN_PROGRESS; next: verify CLI handoff",
            ],
            env,
        )
        run(["obs", "project-a:session", "status: DONE"], env)

        session = graph(["show", "project-a:session"], env)
        assert observations(session, "project-a:session") == ["status: DONE"]

        # Truths: structured key-value attributes, upsert + delete
        run(["truth", "project-a", "language", "rust"], env)
        run(["truth", "project-a", "edition", "2021"], env)
        run(["truth", "project-a", "edition", "2024"], env)  # upsert replaces
        with_truths = graph(["show", "project-a"], env)
        assert truths(with_truths, "project-a") == {
            "language": "rust",
            "edition": "2024",
        }

        run(["rm-truth", "project-a", "language"], env)
        after_truth_delete = graph(["show", "project-a"], env)
        assert truths(after_truth_delete, "project-a") == {"edition": "2024"}

        run(["rm", "UserPreferences"], env)
        after_delete = graph(["show", "project-a", "UserPreferences"], env)
        assert entity_names(after_delete) == {"project-a"}
        assert after_delete["relations"] == []

        # unlink: remove exactly one relation, leaving both endpoints intact.
        run(["link", "project-a", "project-a:session", "tracks"], env)
        linked = graph(["show", "project-a"], env)
        assert ("project-a", "project-a:session", "tracks") in {
            (r["from"], r["to"], r["relationType"]) for r in linked["relations"]
        }
        run(["unlink", "project-a", "project-a:session", "tracks"], env)
        unlinked = graph(["show", "project-a", "project-a:session"], env)
        assert ("project-a", "project-a:session", "tracks") not in {
            (r["from"], r["to"], r["relationType"]) for r in unlinked["relations"]
        }
        assert {"project-a", "project-a:session"}.issubset(entity_names(unlinked))

        # Stats test
        stats = run(["stats"], env).stdout
        assert "Knowledge Graph Statistics" in stats

        # Full export -> reset -> import must preserve entities, truths, and
        # relations (regression guard for JSON round-trip fidelity).
        run(["link", "project-a", "project-a:session", "part_of"], env)
        pre_reset = graph(["graph"], env)
        pre_rels = {
            (r["from"], r["to"], r["relationType"]) for r in pre_reset["relations"]
        }
        assert ("project-a", "project-a:session", "part_of") in pre_rels

        export_file = str(Path(tmp) / "backup.json")
        run(["export", "--output", export_file], env)
        assert Path(export_file).exists()

        run(["reset", "--force"], env)
        empty_graph = graph(["graph"], env)
        assert empty_graph["entities"] == []
        assert empty_graph["relations"] == []

        run(["import", export_file], env)
        restored_graph = graph(["graph"], env)
        assert entity_names(restored_graph) == entity_names(pre_reset)
        assert {
            (r["from"], r["to"], r["relationType"]) for r in restored_graph["relations"]
        } == pre_rels
        # truths survive the JSON round-trip
        restored = graph(["show", "project-a"], env)
        assert truths(restored, "project-a") == {"edition": "2024"}

        # Physical backup -> mutation -> restore must preserve the complete
        # libSQL database. This is distinct from the portable JSON snapshot
        # above and guards the BackupStore CLI wiring.
        snapshot_file = Path(tmp) / "snapshot.db"
        run(["backup", "--output", str(snapshot_file)], env)
        assert snapshot_file.is_file()

        duplicate = run_expect_failure(["backup", "--output", str(snapshot_file)], env)
        assert "already exists" in duplicate.stderr

        run(["reset", "--force"], env)
        assert graph(["graph"], env)["entities"] == []

        run(["restore", str(snapshot_file), "--force"], env)
        physically_restored = graph(["graph"], env)
        assert entity_names(physically_restored) == entity_names(restored_graph)
        assert {
            (r["from"], r["to"], r["relationType"])
            for r in physically_restored["relations"]
        } == pre_rels
        assert truths(physically_restored, "project-a") == {"edition": "2024"}

        safety_backups = list((Path(tmp) / "backups").glob("pre-restore-*.db"))
        assert len(safety_backups) == 1

    with tempfile.TemporaryDirectory(prefix="asobi-corrupt-") as tmp:
        db_path = Path(tmp) / "corrupt.db"
        db_path.write_bytes(b"not a sqlite database")
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(db_path)
        failed = run_expect_failure(["graph"], env)
        assert "database" in failed.stderr.lower()

    batch_and_json_checks()
    skills_checks()
    agent_feature_checks()
    scoped_export_checks()

    print("CLI graph integration checks passed")


def scoped_export_checks() -> None:
    """End-to-end coverage for ``export --scope`` (single-epic handoff bundles).

    Builds two epics under a shared project with a shared pitfall and a decision
    rationale chain, then verifies the directed traversal at the CLI boundary:
    ``part_of`` children are pulled inward, cited ``depends_on`` leaves are
    included but not followed past, the shared pitfall does not drag in the
    sibling epic, the project bridge / session / preferences are excluded, and
    ``--rationale`` adds exactly one ``extends`` hop. Finishes with a round-trip
    ``import`` into a fresh database to prove the bundle is self-contained.
    """
    with tempfile.TemporaryDirectory(prefix="asobi-scope-") as tmp:
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(Path(tmp) / "asobi.db")

        # Two epics under one project, a shared pitfall, and a decision chain.
        run(
            [
                "new",
                "proj",
                "project",
                "proj:a",
                "task",
                "proj:a:task-1",
                "task",
                "proj:a:task-2",
                "task",
                "proj:b",
                "task",
                "proj:b:task-1",
                "task",
                "proj:decision:x",
                "concept",
                "proj:decision:root",
                "concept",
                "proj:pitfall:shared",
                "concept",
                "proj:session",
                "session",
                "UserPreferences",
                "preference",
            ],
            env,
        )
        run(
            [
                "link",
                # epics/tasks belong to their parents (inward part_of)
                "proj:a",
                "proj",
                "part_of",
                "proj:a:task-1",
                "proj:a",
                "part_of",
                "proj:a:task-2",
                "proj:a",
                "part_of",
                "proj:b",
                "proj",
                "part_of",
                "proj:b:task-1",
                "proj:b",
                "part_of",
                # a task cites a decision that extends a root decision
                "proj:a:task-2",
                "proj:decision:x",
                "depends_on",
                "proj:decision:x",
                "proj:decision:root",
                "extends",
                # a pitfall shared by both epics
                "proj:a:task-1",
                "proj:pitfall:shared",
                "depends_on",
                "proj:b:task-1",
                "proj:pitfall:shared",
                "depends_on",
                # a stray edge into the importer's globals (must be guarded out)
                "proj:a:task-1",
                "UserPreferences",
                "depends_on",
            ],
            env,
        )

        # Scope to epic A: children in, cited leaves in, everything else out.
        scoped = entity_names(graph(["export", "--scope", "proj:a"], env))
        assert scoped == {
            "proj:a",
            "proj:a:task-1",
            "proj:a:task-2",
            "proj:decision:x",
            "proj:pitfall:shared",
        }, scoped
        # leaf-terminating: the decision is in, its `extends` target is not
        assert "proj:decision:root" not in scoped
        # shared pitfall must not drag in the sibling epic
        assert "proj:b" not in scoped and "proj:b:task-1" not in scoped
        # project bridge, volatile session, and importer globals excluded
        assert "proj" not in scoped
        assert "proj:session" not in scoped
        assert "UserPreferences" not in scoped

        # --rationale pulls exactly one extends hop off the cited leaf.
        with_rationale = entity_names(
            graph(["export", "--scope", "proj:a", "--rationale"], env)
        )
        assert "proj:decision:root" in with_rationale

        # Multiple roots union without bridging through the project node.
        both = entity_names(
            graph(["export", "--scope", "proj:a", "--scope", "proj:b"], env)
        )
        assert {"proj:a:task-1", "proj:b:task-1"}.issubset(both)
        assert "proj" not in both

        # Round-trip: a scoped bundle imports cleanly into a fresh database.
        bundle = str(Path(tmp) / "epic-a.json")
        run(["export", "--scope", "proj:a", "--output", bundle], env)

        fresh_env = os.environ.copy()
        fresh_env["ASOBI_DATABASE_URL"] = str(Path(tmp) / "fresh.db")
        run(["import", bundle], fresh_env)
        imported = graph(["graph"], fresh_env)
        assert entity_names(imported) == scoped
        # relations survive only when both endpoints are in the bundle
        rel_pairs = {(r["from"], r["to"]) for r in imported["relations"]}
        assert ("proj:a:task-2", "proj:decision:x") in rel_pairs
        assert ("proj:a", "proj") not in rel_pairs


def batch_and_json_checks() -> None:
    """Coverage for batched writes and the global ``--json`` echo.

    ``new`` takes repeated ``NAME TYPE`` pairs and
    ``link`` takes repeated ``FROM TO TYPE`` triples in a single
    call (the underlying DB layer is already batch-capable). ``--json`` makes a
    mutation print the affected entities to stdout so a caller can confirm a
    write without a follow-up ``show``.
    """
    with tempfile.TemporaryDirectory(prefix="asobi-batch-") as tmp:
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(Path(tmp) / "asobi.db")

        # new: one call, multiple NAME TYPE pairs.
        run(
            ["new", "alpha", "task", "beta", "concept", "gamma", "ref"],
            env,
        )
        assert entity_names(graph(["graph"], env)) == {"alpha", "beta", "gamma"}

        # Argument count not a multiple of 2 is rejected with a clear message.
        bad_pairs = run_expect_failure(["new", "x", "task", "y"], env)
        assert "pair" in bad_pairs.stderr.lower()

        # link: one call, multiple FROM TO TYPE triples.
        run(
            ["link", "alpha", "beta", "uses", "alpha", "gamma", "blocks"],
            env,
        )
        rels = graph(["graph"], env)["relations"]
        assert {(r["from"], r["to"], r["relationType"]) for r in rels} == {
            ("alpha", "beta", "uses"),
            ("alpha", "gamma", "blocks"),
        }

        # Argument count not a multiple of 3 is rejected.
        bad_triples = run_expect_failure(["link", "a", "b", "uses", "c"], env)
        assert "triple" in bad_triples.stderr.lower()

        # --json: new echoes the created entity to stdout.
        echoed = json.loads(run(["new", "delta", "task", "--json"], env).stdout)
        assert "delta" in entity_names(echoed)

        # --json: obs returns the affected entity with its trail.
        obs_echo = json.loads(run(["obs", "delta", "first obs", "--json"], env).stdout)
        assert observations(obs_echo, "delta") == ["first obs"]

        # --json: link shows the relation among its endpoints.
        rel_echo = json.loads(
            run(["link", "delta", "alpha", "uses", "--json"], env).stdout
        )
        assert {
            "from": "delta",
            "to": "alpha",
            "relationType": "uses",
        } in rel_echo["relations"]

        # --json: truth / rm-truth return the entity's current truths.
        truth_echo = json.loads(
            run(["truth", "delta", "status", "READY", "--json"], env).stdout
        )
        assert truths(truth_echo, "delta") == {"status": "READY"}

        # --json: rm reports the removed names (entities are gone,
        # so there is nothing to open — the shape is a deletion receipt).
        del_echo = json.loads(run(["rm", "gamma", "--json"], env).stdout)
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


def agent_feature_checks() -> None:
    """Coverage for the new agent-centric features:

    - rm-obs --prefix
    - update-obs
    - show --expand and --with-timestamps
    - stats --per-entity
    - JSON error formatting
    """
    with tempfile.TemporaryDirectory(prefix="asobi-agent-") as tmp:
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(Path(tmp) / "asobi.db")

        # 1. new and obs
        run(["new", "alice", "person", "bob", "person"], env)
        run(
            ["obs", "alice", "status: active", "next: code", "old info"],
            env,
        )
        run(["link", "alice", "bob", "follows"], env)

        # 2. show --with-ids to get IDs
        shown = graph(["show", "alice", "--with-ids"], env)
        detailed = shown["entities"][0]["observationsDetailed"]
        assert detailed[0]["id"] == 1
        assert detailed[0]["content"] == "status: active"
        assert detailed[2]["id"] == 3
        assert detailed[2]["content"] == "old info"

        # 3. rm-obs with --id
        run(["rm-obs", "alice", "1", "--id"], env)

        # 4. update-obs with --id
        run(["update-obs", "alice", "3", "new info", "--id"], env)

        # 4b. verify changes with show --with-ids
        shown = graph(["show", "alice", "--with-ids"], env)
        detailed = shown["entities"][0]["observationsDetailed"]
        contents = {o["content"] for o in detailed}
        assert contents == {"next: code", "new info"}

        # 5. show --expand
        expanded = graph(["show", "alice", "--expand", "follows"], env)
        names = {e["name"] for e in expanded["entities"]}
        assert names == {"alice", "bob"}

        # 6. stats --per-entity
        stats_out = run(["stats", "--per-entity"], env).stdout
        assert "Entities by Observation Count:" in stats_out
        assert "alice" in stats_out

        # 6b. stats --json --per-entity
        stats_json = json.loads(run(["--json", "stats", "--per-entity"], env).stdout)
        assert stats_json["entities"] == 2
        assert stats_json["relations"] == 1
        assert stats_json["entitiesDetailed"][0]["name"] == "alice"

        # 7. JSON error formatting
        failed = run_expect_failure(["--json", "import", "nonexistent_abc.json"], env)
        err_json = json.loads(failed.stdout)
        assert err_json["status"] == "failed"
        assert "No such file or directory" in err_json["error"]


if __name__ == "__main__":
    main()
