#!/usr/bin/env python3
"""Document-tier CLI integration checks for Asobi (requires ``--features documents``).

Companion to ``verify_cli.py`` (which covers the graph tier). This script drives
the ``ingest`` / ``query`` / ``compact`` surface through the built binary and
verifies the behaviour that only exists with the document feature compiled in:

* ``compact`` persists *every* knowledge entity type to a Markdown topic and
  skips the graph-only types — volatile ``session`` / ``task`` state plus
  ``skill`` (already indexed by its installer) — the denylist contract in
  ``compact::should_sync`` (regression guard: ``preference`` must persist).
* Compacted topic frontmatter is YAML-quoted and round-trips through the shared
  ``frontmatter`` parser.
* ``compact`` is idempotent — a second run rewrites the same files in place
  (``File::create`` truncates), it never refuses to override.
* ``ingest`` indexes a Markdown file so ``query`` recalls it, and a synced topic
  is itself recall-able after ``compact``.

Isolation: each case points ``XDG_DATA_HOME`` at a fresh temp dir so the unified
root (``$XDG_DATA_HOME/asobi/{data,topics}``) co-locates the db and topics — no
``ASOBI_HOME`` (which flattens the layout) and no ``ASOBI_DATABASE_URL`` (which
would split the db from the resolved topics dir). Run via
``make test-documents-scripts`` (uv).
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "asobi"

# Mirror of compact::should_sync. Knowledge types are written to a topic file;
# session/task are volatile state and skill is already indexed by its installer,
# so all three stay graph-only.
KNOWLEDGE_TYPES = ("project", "concept", "reference", "preference", "standard")
SKIPPED_TYPES = ("session", "task", "skill")


def doc_env(home: Path) -> dict[str, str]:
    """Env that isolates the document tier under one XDG root."""
    env = os.environ.copy()
    # Force the unified XDG layout; drop anything that would redirect the db or
    # topics elsewhere and desync them.
    env.pop("ASOBI_HOME", None)
    env.pop("ASOBI_DATABASE_URL", None)
    env.pop("ASOBI_TOPICS_DIR", None)
    env["XDG_DATA_HOME"] = str(home)
    return env


def topics_dir(home: Path) -> Path:
    return home / "asobi" / "topics"


def run(
    args: list[str], env: dict[str, str], cwd: Path
) -> subprocess.CompletedProcess[str]:
    # cwd must sit *outside* any asobi workspace (the repo root carries its own
    # asobi.toml) so path resolution falls through to the isolated XDG root.
    result = subprocess.run(
        [str(BIN), *args],
        cwd=cwd,
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


def slugify(name: str) -> str:
    """Port of normalize::slugify for predicting topic filenames."""
    slug = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")
    return slug


def main() -> None:
    subprocess.run(["cargo", "build", "--features", "documents"], cwd=ROOT, check=True)

    compact_persists_every_type()
    compact_is_idempotent()
    ingest_then_query_recalls()
    synced_topic_is_recallable()

    print("Document CLI integration checks passed")


def compact_persists_every_type() -> None:
    """Every knowledge type lands a topic; session/task/skill never do.

    Regression guard for the reported "preference didn't persist" bug: the
    denylist is exactly {session, task, skill}, so each knowledge type —
    ``preference`` included — must produce a file, and nothing else may.
    """
    with tempfile.TemporaryDirectory(prefix="asobi-doc-types-") as tmp:
        home = Path(tmp)
        env = doc_env(home)

        for etype in (*KNOWLEDGE_TYPES, *SKIPPED_TYPES):
            name = f"ent:{etype}"
            run(["new", name, etype], env, home)
            run(["obs", name, f"trail note for a {etype} entity"], env, home)

        run(["compact"], env, home)

        present = {p.name for p in topics_dir(home).glob("*.md")}
        expected = {f"{slugify(f'ent:{t}')}.md" for t in KNOWLEDGE_TYPES}
        skipped = {f"{slugify(f'ent:{t}')}.md" for t in SKIPPED_TYPES}

        missing = expected - present
        assert not missing, (
            f"knowledge types not persisted by compact: {sorted(missing)}\n"
            f"topics present: {sorted(present)}"
        )
        leaked = skipped & present
        assert not leaked, f"graph-only state wrongly synced: {sorted(leaked)}"
        assert present == expected, (
            f"unexpected topic set.\nexpected: {sorted(expected)}\n"
            f"present:  {sorted(present)}"
        )

        # Frontmatter is YAML-quoted so a ':'-bearing name stays strict-YAML
        # valid, and the body still carries the observation trail.
        pref = (topics_dir(home) / f"{slugify('ent:preference')}.md").read_text()
        assert 'title: "ent:preference"' in pref, f"title not quoted:\n{pref}"
        assert 'type: "preference"' in pref, f"type not quoted:\n{pref}"
        assert "trail note for a preference entity" in pref


def compact_is_idempotent() -> None:
    """Re-compact is stable: a no-op leaves the file untouched, a change rewrites it.

    Directly exercises the original "second compact won't override file" report.
    An unchanged entity must be left byte-for-byte (the ``compacted`` timestamp is
    preserved, not bumped), while a mutated entity is rewritten in place.
    """
    with tempfile.TemporaryDirectory(prefix="asobi-doc-idem-") as tmp:
        home = Path(tmp)
        env = doc_env(home)
        topic = topics_dir(home) / f"{slugify('proj:demo')}.md"

        run(["new", "proj:demo", "project"], env, home)
        run(["obs", "proj:demo", "first observation"], env, home)
        run(["compact"], env, home)
        first = topic.read_text()
        assert "first observation" in first

        # No-op re-compact: nothing changed in the graph, so the topic — its
        # `compacted` timestamp included — must be preserved exactly.
        run(["compact"], env, home)
        assert topic.read_text() == first, "no-op compact rewrote an unchanged topic"

        # Mutate the graph, compact again: the file is overwritten with the new
        # state (old line gone, new line present), proving the rewrite happens.
        run(["truth", "proj:demo", "status", "ACTIVE"], env, home)
        run(["obs", "proj:demo", "second observation"], env, home)
        run(["compact"], env, home)
        second = topic.read_text()

        assert second != first, "changed entity not rewritten"
        assert "second observation" in second, "second compact did not override"
        assert 'truth_status: "ACTIVE"' in second, "new truth not reflected"
        assert {p.name for p in topics_dir(home).glob("*.md")} == {topic.name}


def ingest_then_query_recalls() -> None:
    """``ingest`` indexes a file so ``query`` recalls it by a distinctive term."""
    with tempfile.TemporaryDirectory(prefix="asobi-doc-ingest-") as tmp:
        home = Path(tmp)
        env = doc_env(home)

        doc = home / "note.md"
        doc.write_text(
            "---\ntitle: Auth Design\n---\n\n"
            "Session tokens are verified with the zephyrquux handshake.\n"
        )
        run(["ingest", str(doc)], env, home)

        results = json.loads(
            run(["query", "zephyrquux handshake", "--json"], env, home).stdout
        )
        assert results, "ingested document not recalled by query"
        assert any("zephyrquux" in r["snippet"].lower() for r in results), (
            f"distinctive term missing from recall snippets: {results}"
        )


def synced_topic_is_recallable() -> None:
    """A topic written by ``compact`` is itself queryable from the document tier."""
    with tempfile.TemporaryDirectory(prefix="asobi-doc-synced-") as tmp:
        home = Path(tmp)
        env = doc_env(home)

        run(["new", "UserPreferences", "preference"], env, home)
        run(
            ["obs", "UserPreferences", "Prefers the qubefable recall workflow."],
            env,
            home,
        )
        run(["compact"], env, home)

        results = json.loads(
            run(["query", "qubefable recall", "--json"], env, home).stdout
        )
        assert results, "synced preference topic not recalled after compact"
        assert any(
            "userpreferences" in r["topicId"].lower()
            or "qubefable" in r["snippet"].lower()
            for r in results
        ), f"compacted preference not found in recall: {results}"


if __name__ == "__main__":
    main()
