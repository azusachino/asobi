#!/usr/bin/env python3
"""Verify the real CLI path against the versioned API and Turso backend."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "asobi"


def run(
    args: list[str], env: dict[str, str], expect_success: bool = True
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        [str(BIN), *args],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0 and expect_success:
        raise AssertionError(
            f"command failed: asobi {' '.join(args)}\n"
            f"stdout:\n{result.stdout}\n stderr:\n{result.stderr}"
        )
    if result.returncode == 0 and not expect_success:
        raise AssertionError(f"command unexpectedly succeeded: asobi {' '.join(args)}")
    return result


def main() -> None:
    # Turso is opt-in: it must be compiled behind the feature and selected at
    # runtime with ASOBI_BACKEND=turso (libSQL is the default provider).
    subprocess.run(
        ["cargo", "build", "--features", "turso-experimental"], cwd=ROOT, check=True
    )

    with tempfile.TemporaryDirectory(prefix="asobi-turso-cli-") as tmp:
        root = Path(tmp)
        env = os.environ.copy()
        env["ASOBI_BACKEND"] = "turso"
        env["ASOBI_DATABASE_URL"] = str(root / "asobi.db")

        capabilities = json.loads(run(["capabilities"], env).stdout)
        assert capabilities["apiVersion"] == 1
        assert capabilities["capabilities"]["backend"] == "turso"
        # Keyword search is correct via a stable substring scan (no native FTS).
        assert capabilities["capabilities"]["keywordSearch"] is True
        assert capabilities["health"] == {
            "backend": "turso",
            "reachable": True,
            "detail": None,
        }

        run(["new", "turso-project", "project"], env)
        run(["obs", "turso-project", "Turso substring search verification"], env)
        search = json.loads(run(["search", "Turso"], env).stdout)
        assert {entity["name"] for entity in search["entities"]} == {"turso-project"}

        fresh_home = root / "fresh-home"
        fresh_home.mkdir()
        (fresh_home / "asobi.db").write_bytes(b"legacy placeholder")
        fresh_env = os.environ.copy()
        fresh_env.pop("ASOBI_DATABASE_URL", None)
        fresh_env["ASOBI_BACKEND"] = "turso"
        fresh_env["ASOBI_HOME"] = str(fresh_home)
        initialized = run(["capabilities"], fresh_env)
        assert (fresh_home / "asobi.turso.db").exists()
        assert "found an older Asobi database" in initialized.stderr

    print("Turso API/CLI integration checks passed")


if __name__ == "__main__":
    main()
