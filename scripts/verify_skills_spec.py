#!/usr/bin/env python3
"""Check that `asobi skills` writes spec-conformant Agent Skills.

Runs the Agent Skills reference validator (`skills-ref`, the implementation
published alongside the specification) against skills Asobi materialises, so a
drift from the spec fails `make check` instead of surfacing later in whichever
agent host tries to load the result.

One divergence is deliberate and therefore excluded here. The spec requires a
skill's directory name to equal its frontmatter `name`, which assumes a skill is
authored in place. Asobi installs *many sources* into one tree and names each
directory `<source-slug>@<skill-name>`, because two sources may ship the same
skill name and because agent hosts surface that directory name as the skill's
identity -- flattening to a bare name would both collide and silently rename
every installed skill. So each skill's content is validated in a directory named
to match it, which checks everything the spec says about a skill *as a skill*
while leaving the multi-source naming to Asobi.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SKILLS_REF = "skills-ref@0.1.5"


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


def validate(skill_dir: Path, name: str, workdir: Path) -> tuple[bool, str]:
    """Validate one skill's content under a spec-conformant directory name."""
    staged = workdir / name
    if staged.exists():
        shutil.rmtree(staged)
    shutil.copytree(skill_dir, staged)
    result = subprocess.run(
        ["bun", "x", SKILLS_REF, "validate", str(staged)],
        capture_output=True,
        text=True,
        cwd=workdir,
    )
    return result.returncode == 0, (result.stdout + result.stderr).strip()


def main() -> None:
    skills_root = REPO / ".agents" / "skills"
    installed = sorted(d for d in skills_root.glob("*@*") if (d / "SKILL.md").is_file())
    if not installed:
        print(f"no installed skills under {skills_root}; nothing to validate")
        return

    manifest = skills_root / ".asobi-skills.json"
    if not manifest.is_file():
        sys.exit(f"missing provenance manifest: {manifest}")
    recorded = {entry["dir"] for entry in json.loads(manifest.read_text())["skills"]}
    if orphans := {d.name for d in installed} - recorded:
        sys.exit(f"installed but absent from the manifest: {sorted(orphans)}")

    failures = []
    with tempfile.TemporaryDirectory(prefix="asobi-skills-spec-") as tmp:
        workdir = Path(tmp)
        for skill_dir in installed:
            name = frontmatter_name(skill_dir / "SKILL.md")
            if not name:
                failures.append(f"{skill_dir.name}: no frontmatter `name:`")
                continue
            ok, output = validate(skill_dir, name, workdir)
            print(f"  {'✓' if ok else '✗'} {skill_dir.name} (name: {name})")
            if not ok:
                failures.append(f"{skill_dir.name}: {output}")

    if failures:
        sys.exit(
            "\nAgent Skills spec violations:\n" + "\n".join(f"  {f}" for f in failures)
        )
    print(f"agent skills spec: {len(installed)} skill(s) conform")


if __name__ == "__main__":
    main()
