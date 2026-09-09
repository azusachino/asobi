#!/usr/bin/env python3
"""Check that `asobi skills` writes spec-conformant Agent Skills.

Installs a fixture skill with the built CLI and runs the Agent Skills reference
validator (`skills-ref`, published alongside the specification) over what lands
on disk, so a drift from the spec fails `make check` instead of surfacing later
in whichever agent host tries to load the result.

Deliberately a fixture rather than whatever a contributor happens to have
installed: skills under `.agents/skills/` belong to the person who installed
them, and this repository ships none of its own.

One divergence is excluded, intentionally. The spec requires a skill's directory
name to equal its frontmatter `name`, which assumes a skill is authored in
place. Asobi installs *many sources* into one tree and names each directory
`<source-slug>@<skill-name>`, because two sources may ship the same skill name
and because agent hosts surface that directory name as the skill's identity --
flattening to a bare name would both collide and silently rename every installed
skill. The content is therefore validated under a directory named to match it,
which checks everything the spec says about a skill *as a skill* while leaving
the multi-source naming to Asobi.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "asobi"
SKILLS_REF = "skills-ref@0.1.5"

# Markdown a skill points at must survive installation; everything else must
# not. Auxiliary artifacts are where the published attack research finds
# payloads hidden, so an install writes instructions and nothing executable.
INSTALLED = ("references/REFERENCE.md",)
WITHHELD = ("scripts/run.sh", "assets/table.json")

FIXTURE = {
    "SKILL.md": (
        "---\n"
        "name: spec-fixture\n"
        "description: A fixture skill used to verify that Asobi writes spec-conformant output.\n"
        "license: MIT\n"
        "metadata:\n"
        "  author: asobi\n"
        '  version: "1.0"\n'
        "---\n\n"
        "# Spec fixture\n\n"
        "See [the reference](references/REFERENCE.md) and run `scripts/run.sh`.\n"
    ),
    "references/REFERENCE.md": "# Reference\n\nLoaded on demand, not inlined.\n",
    "scripts/run.sh": "#!/bin/sh\necho fixture\n",
    "assets/table.json": '{"kind": "asset"}\n',
}


def write_fixture(root: Path) -> Path:
    """A skill that owns a directory and ships one file of each optional kind."""
    skill = root / "spec-fixture"
    for relative, content in FIXTURE.items():
        target = skill / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")
    return skill


def frontmatter_name(skill_md: Path) -> str | None:
    """The declared `name:`, read without a YAML dependency.

    Deliberately literal: the spec requires frontmatter to open at byte 0 with
    `---`, so anything else is malformed and worth reporting as such.
    """
    text = skill_md.read_text(encoding="utf-8")
    if not text.startswith("---"):
        return None
    _, _, rest = text.partition("---\n")
    body, _, _ = rest.partition("\n---")
    for line in body.splitlines():
        key, sep, value = line.partition(":")
        if sep and key.strip() == "name":
            return value.strip().strip("\"'")
    return None


def install_fixture(work: Path) -> list[Path]:
    """Install the fixture into an isolated workspace and return what landed."""
    source = write_fixture(work / "source")
    home = work / "home"
    home.mkdir()

    env = os.environ.copy()
    env["ASOBI_HOME"] = str(home)
    env["ASOBI_DATABASE_URL"] = str(work / "asobi.db")
    install = subprocess.run(
        [str(BIN), "skills", "install", str(source), "--all"],
        capture_output=True,
        text=True,
        env=env,
        check=False,
    )
    if install.returncode != 0:
        sys.exit(f"skills install failed:\n{install.stdout}{install.stderr}")

    skills_root = home / ".agents" / "skills"
    installed = sorted(d for d in skills_root.glob("*@*") if (d / "SKILL.md").is_file())
    if not installed:
        sys.exit(f"install wrote nothing under {skills_root}")

    # The manifest is state, so it sits in the data directory (here `home`,
    # since ASOBI_HOME unifies the roots) rather than beside the skills, and is
    # named for the skills directory it describes. Glob rather than recompute
    # the key: this checks that a manifest was written, not how it is named.
    manifests = list(home.glob("skills-*.json"))
    if len(manifests) != 1:
        sys.exit(
            f"expected exactly one provenance manifest under {home}, found {manifests}"
        )
    recorded = {
        entry["dir"] for entry in json.loads(manifests[0].read_text())["skills"]
    }
    if orphans := {d.name for d in installed} - recorded:
        sys.exit(f"installed but absent from the manifest: {sorted(orphans)}")
    return installed


def main() -> None:
    if not BIN.is_file():
        sys.exit(f"missing CLI at {BIN}; run `make build` first")

    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="asobi-skills-spec-") as tmp:
        work = Path(tmp)
        installed = install_fixture(work)

        for skill_dir in installed:
            # Referenced Markdown must survive installation: the spec loads it
            # on demand when the body points at it, so a body referencing a file
            # that is not there is a skill that breaks only once used.
            failures += [
                f"{skill_dir.name}: referenced {relative} was not installed"
                for relative in INSTALLED
                if not (skill_dir / relative).is_file()
            ]
            failures += [
                f"{skill_dir.name}: {relative} was installed; only Markdown should be"
                for relative in WITHHELD
                if (skill_dir / relative).exists()
            ]

            name = frontmatter_name(skill_dir / "SKILL.md")
            if not name:
                failures.append(f"{skill_dir.name}: no frontmatter `name:`")
                continue

            staged = work / "staged" / name
            staged.parent.mkdir(parents=True, exist_ok=True)
            shutil.copytree(skill_dir, staged, dirs_exist_ok=True)
            result = subprocess.run(
                ["bun", "x", SKILLS_REF, "validate", str(staged)],
                capture_output=True,
                text=True,
                cwd=work,
                check=False,
            )
            ok = result.returncode == 0
            print(f"  {'✓' if ok else '✗'} {skill_dir.name} (name: {name})")
            if not ok:
                failures.append(
                    f"{skill_dir.name}: {(result.stdout + result.stderr).strip()}"
                )

        count = len(installed)

    if failures:
        sys.exit(
            "\nAgent Skills spec violations:\n" + "\n".join(f"  {f}" for f in failures)
        )
    print(
        f"agent skills spec: {count} skill(s) conform, Markdown intact, executables withheld"
    )


if __name__ == "__main__":
    main()
