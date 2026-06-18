#!/usr/bin/env python3
"""Use-case scenarios showing Asobi's performance and correctness for task boarding."""

import json
import os
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target" / "debug" / "asobi"


def run_cmd(args: list[str], db_path: str) -> str:
    env = os.environ.copy()
    env["ASOBI_DATABASE_URL"] = db_path
    
    start = time.perf_counter()
    result = subprocess.run(
        [str(BIN), *args],
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=True
    )
    elapsed = time.perf_counter() - start
    # Print the command and elapsed time for performance tracing
    cmd_str = f"asobi {' '.join(args)}"
    print(f"[PERF] {cmd_str:<60} | {elapsed * 1000:6.2f} ms")
    return result.stdout


def main() -> None:
    # Build the binary first to make sure it's up to date
    print("Building target/debug/asobi...")
    subprocess.run(["cargo", "build"], cwd=ROOT, check=True)
    
    with tempfile.TemporaryDirectory(prefix="asobi-usecase-") as tmp_dir:
        db_path = str(Path(tmp_dir) / "usecase.db")
        print(f"\n--- Initializing Usecase DB at {db_path} ---")
        
        # -------------------------------------------------------------
        # Use-Case 1: Seed Epic and Tasks (Using new --obs and truths)
        # -------------------------------------------------------------
        print("\n--- Use-Case 1: Seeding Epic and Tasks atomically ---")
        
        # Seed the epic
        run_cmd(["new", "asobi:v0.3", "task", "--obs", "Epic for v0.3 release"], db_path)
        run_cmd(["truth", "asobi:v0.3", "status", "IN_PROGRESS"], db_path)
        
        # Seed 3 tasks in the epic with initial statuses and observations
        run_cmd([
            "new", 
            "asobi:v0.3:task-1", "task", 
            "--obs", "Implement atomic bulk backup", 
            "asobi:v0.3:task-2", "task", 
            "--obs", "Optimize index page caching",
            "asobi:v0.3:task-3", "task", 
            "--obs", "Write end-to-end integration tests"
        ], db_path)
        
        # Set task statuses and blockages
        run_cmd(["truth", "asobi:v0.3:task-1", "status", "READY_TO_DISPATCH"], db_path)
        run_cmd(["truth", "asobi:v0.3:task-2", "status", "BLOCKED"], db_path)
        run_cmd(["truth", "asobi:v0.3:task-2", "blocked_on", "asobi:v0.3:task-1"], db_path)
        run_cmd(["truth", "asobi:v0.3:task-3", "status", "READY_TO_DISPATCH"], db_path)
        
        # Link tasks to the epic
        run_cmd([
            "link", 
            "asobi:v0.3:task-1", "asobi:v0.3", "part_of",
            "asobi:v0.3:task-2", "asobi:v0.3", "part_of",
            "asobi:v0.3:task-3", "asobi:v0.3", "part_of"
        ], db_path)
        
        # -------------------------------------------------------------
        # Use-Case 2: Board Search (Search by truth status only)
        # -------------------------------------------------------------
        print("\n--- Use-Case 2: Board Search (status=READY_TO_DISPATCH) ---")
        
        stdout = run_cmd(["search", "--where", "status=READY_TO_DISPATCH"], db_path)
        graph = json.loads(stdout)
        
        matched_names = {e["name"] for e in graph["entities"] if e.get("truths", {}).get("status") == "READY_TO_DISPATCH"}
        print(f"Matched READY_TO_DISPATCH entities: {matched_names}")
        assert matched_names == {"asobi:v0.3:task-1", "asobi:v0.3:task-3"}, f"Mismatch: {matched_names}"
        
        # -------------------------------------------------------------
        # Use-Case 3: Dispatch and Update Status
        # -------------------------------------------------------------
        print("\n--- Use-Case 3: Claim and complete task-1 ---")
        
        # Claim task-1
        run_cmd(["truth", "asobi:v0.3:task-1", "status", "IN_PROGRESS"], db_path)
        run_cmd(["obs", "asobi:v0.3:task-1", "Assigned to agent-antigravity"], db_path)
        
        # Complete task-1
        run_cmd(["truth", "asobi:v0.3:task-1", "status", "DONE"], db_path)
        run_cmd(["obs", "asobi:v0.3:task-1", "Completed migration check, code is fully functional."], db_path)
        
        # Verify status is updated
        stdout_show = run_cmd(["show", "asobi:v0.3:task-1"], db_path)
        show_graph = json.loads(stdout_show)
        entity = show_graph["entities"][0]
        assert entity["truths"]["status"] == "DONE"
        assert "Completed migration check, code is fully functional." in entity["observations"]
        
        print("\nAll integration use-cases successfully validated!")


if __name__ == "__main__":
    main()
