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

import fastjsonschema

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "asobi"
_SCHEMAS: dict[str, dict] = {}
_SCHEMA_FORMATS = {
    "uint": lambda value: isinstance(value, int) and value >= 0,
    "uint32": lambda value: isinstance(value, int) and 0 <= value <= 2**32 - 1,
    "int64": lambda value: isinstance(value, int),
    "double": lambda value: isinstance(value, (int, float)),
}


def run(
    args: list[str], env: dict[str, str], cwd: Path | None = None
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        [str(BIN), *args],
        cwd=cwd or ROOT,
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
    args: list[str], env: dict[str, str], cwd: Path | None = None
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        [str(BIN), *args],
        cwd=cwd or ROOT,
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
    command = args[0]
    return validate_response(args, env, command)


def validate_response(args: list[str], env: dict[str, str], command: str) -> dict:
    payload = json.loads(run(args, env).stdout)
    schema = command_schema(command, env)
    fastjsonschema.compile(schema, formats=_SCHEMA_FORMATS)(payload)
    return payload


def command_schema(command: str, env: dict[str, str]) -> dict:
    if command not in _SCHEMAS:
        _SCHEMAS[command] = json.loads(
            run(["schema", "--command", command], env).stdout
        )
    return _SCHEMAS[command]


def validate_error(payload: dict) -> dict:
    assert payload["status"] == "failed"
    assert isinstance(payload["error"], str)
    return payload


def json_data(args: list[str], env: dict[str, str]) -> dict:
    """Return the data payload from a successful versioned response."""
    commands = {
        "capabilities",
        "graph",
        "link",
        "new",
        "obs",
        "rm",
        "rm-obs",
        "rm-truth",
        "search",
        "show",
        "stats",
        "truth",
        "unlink",
        "update-obs",
    }
    command = next(arg for arg in args if arg in commands)
    return validate_response(args, env, command)


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


def schema_checks(env: dict[str, str]) -> None:
    index = json.loads(run(["schema"], env).stdout)
    assert index["schemaVersion"] == 1
    assert "commands" in index
    assert "graph" in index["commands"]
    assert "properties" in index["commands"]["graph"]

    graph_schema = json.loads(run(["schema", "--command", "graph"], env).stdout)
    fastjsonschema.compile(graph_schema, formats=_SCHEMA_FORMATS)
    assert graph_schema["x-asobi-schema-version"] == 1


def main() -> None:
    subprocess.run(["cargo", "build"], cwd=ROOT, check=True)
    schema_checks(os.environ.copy())

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
                "Uses SQLite FTS5 for graph recall.",
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

        keyword_match = graph(["search", "SQLite"], env)
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

        # Reset clears the graph.
        run(["link", "project-a", "project-a:session", "part_of"], env)
        assert graph(["graph"], env)["relations"]
        run(["reset", "--force"], env)
        empty_graph = graph(["graph"], env)
        assert empty_graph["entities"] == []
        assert empty_graph["relations"] == []

    with tempfile.TemporaryDirectory(prefix="asobi-corrupt-") as tmp:
        db_path = Path(tmp) / "corrupt.db"
        db_path.write_bytes(b"not a sqlite database")
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(db_path)
        failed = run_expect_failure(["graph"], env)
        assert "database" in failed.stderr.lower()

    batch_and_json_checks()
    agent_feature_checks()
    task_checks()

    print("CLI graph integration checks passed")


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
        echoed = json_data(["new", "delta", "task", "--json"], env)
        assert "delta" in entity_names(echoed)

        # --json: obs returns the affected entity with its trail.
        obs_echo = json_data(["obs", "delta", "first obs", "--json"], env)
        assert observations(obs_echo, "delta") == ["first obs"]

        # --json: link shows the relation among its endpoints.
        rel_echo = json_data(["link", "delta", "alpha", "uses", "--json"], env)
        assert {
            "from": "delta",
            "to": "alpha",
            "relationType": "uses",
        } in rel_echo["relations"]

        # --json: truth / rm-truth return the entity's current truths.
        truth_echo = json_data(["truth", "delta", "status", "READY", "--json"], env)
        assert truths(truth_echo, "delta") == {"status": "READY"}

        # --json: rm reports the removed names (entities are gone,
        # so there is nothing to open — the shape is a deletion receipt).
        del_echo = json_data(["rm", "gamma", "--json"], env)
        assert del_echo == {"deleted": ["gamma"]}


def task_checks() -> None:
    """Exercise task planning, lifecycle transitions, and rejection paths."""
    with tempfile.TemporaryDirectory(prefix="asobi-tasks-") as tmp:
        env = os.environ.copy()
        env["ASOBI_DATABASE_URL"] = str(Path(tmp) / "asobi.db")

        for help_args in [
            ["--help"],
            ["tasks", "--help"],
            ["tasks", "plan", "--help"],
            ["tasks", "list", "--help"],
            ["tasks", "dispatch", "--help"],
            ["tasks", "sync", "--help"],
            ["tasks", "close", "--help"],
        ]:
            assert run(help_args, env).returncode == 0

        for round_no in range(3):
            epic = f"verify:tasks-{round_no}"
            task_1 = f"{epic}:task-1"
            task_2 = f"{epic}:task-2"
            planned = validate_response(
                [
                    "tasks",
                    "plan",
                    epic,
                    "--objective",
                    "verify task lifecycle",
                    "--task",
                    "first task",
                    "--task",
                    "second task",
                    "--json",
                ],
                env,
                "tasks-plan",
            )
            assert {e["name"] for e in planned["entities"]} == {
                epic,
                task_1,
                task_2,
            }
            board = validate_response(["tasks", "list", epic], env, "tasks-list")
            assert board["entities"][1]["truths"]["status"] == "READY_TO_DISPATCH"

            duplicate = run_expect_failure(
                [
                    "tasks",
                    "plan",
                    epic,
                    "--objective",
                    "duplicate",
                    "--task",
                    "should fail",
                ],
                env,
            )
            assert "already exists" in duplicate.stderr

            dispatch = validate_response(
                ["tasks", "dispatch", "--json"], env, "tasks-dispatch"
            )
            assert dispatch["status"] == "DISPATCHED"
            not_ready = run_expect_failure(
                ["tasks", "dispatch", dispatch["entity"]], env
            )
            assert "READY_TO_DISPATCH" in not_ready.stderr

            invalid_status = run_expect_failure(
                ["tasks", "sync", task_1, "--status", "NOT_A_STATUS"], env
            )
            assert "invalid task status" in invalid_status.stderr
            missing = run_expect_failure(
                ["tasks", "sync", "verify:missing", "--status", "DONE"], env
            )
            assert "not found" in missing.stderr
            early_close = run_expect_failure(["tasks", "close", epic], env)
            assert "every child task" in early_close.stderr

            validate_response(
                ["tasks", "sync", task_1, "--status", "DONE", "--json"],
                env,
                "tasks-sync",
            )
            validate_response(
                ["tasks", "sync", task_2, "--status", "DONE", "--json"],
                env,
                "tasks-sync",
            )
            closed = validate_response(
                ["tasks", "close", epic, "--json"], env, "tasks-close"
            )
            assert closed["status"] == "DONE"


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
        stats_json = json_data(["--json", "stats", "--per-entity"], env)
        assert stats_json["entities"] == 2
        assert stats_json["relations"] == 1
        assert stats_json["entitiesDetailed"][0]["name"] == "alice"

        # 7. JSON error formatting
        failed = run_expect_failure(
            ["--json", "tasks", "close", "no_such_epic_abc"], env
        )
        err_json = validate_error(json.loads(failed.stdout))
        assert "not found" in err_json["error"]


if __name__ == "__main__":
    main()
