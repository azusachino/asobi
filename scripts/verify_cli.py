import subprocess
import os
import shutil


def run_cmd(args):
    return subprocess.run(args, capture_output=True, text=True)


def main():
    # 0. Clean workspace
    if os.path.exists(".rosemary"):
        shutil.rmtree(".rosemary")
    os.makedirs(".rosemary/topics", exist_ok=True)
    subprocess.run(["cargo", "build"], check=True)
    exe = "./target/debug/rosemary"

    # --- Scenario: Project A Context Handoff ---
    project_id = "project-a"
    session_ent = f"{project_id}:session"
    tasks_ent = f"{project_id}:tasks"
    obs_ent = f"{project_id}:observations"

    # 1. Agent 1: Setup project context
    print("\n--- Agent 1: Initialize Project A Context ---")
    run_cmd([exe, "create-entities", session_ent, "session"])
    run_cmd([exe, "create-entities", tasks_ent, "tasks"])
    run_cmd([exe, "create-entities", obs_ent, "observations"])

    run_cmd(
        [
            exe,
            "add-observations",
            tasks_ent,
            "T-001: Implement CLI refactor - Status: DONE",
        ]
    )
    run_cmd(
        [
            exe,
            "add-observations",
            tasks_ent,
            "T-002: Add graph primitives - Status: IN_PROGRESS",
        ]
    )
    run_cmd(
        [
            exe,
            "add-observations",
            obs_ent,
            "Performance: SQLite locking observed during concurrent tests.",
        ]
    )

    # Global preferences
    run_cmd([exe, "create-entities", "UserPreferences", "preference"])
    run_cmd(
        [
            exe,
            "add-observations",
            "UserPreferences",
            "Avoid using unnecessary subcommands",
        ]
    )

    run_cmd([exe, "compact"])
    print("Agent 1: Context saved to shared graph")

    # 2. Simulate "Agent Restart" / New Session
    # We don't delete .rosemary, we just act as a new Agent B calling query/list
    print("\n--- Agent 2: Resuming Context ---")

    # Verify Task list retrieval
    res = run_cmd([exe, "query", "T-002"])
    assert "T-002" in res.stdout and "IN_PROGRESS" in res.stdout

    # Verify Observations
    res = run_cmd([exe, "query", "SQLite locking"])
    assert "Performance" in res.stdout

    # Verify Preferences
    res = run_cmd([exe, "query", "UserPreferences"])
    assert "Avoid using unnecessary subcommands" in res.stdout

    # 3. Agent 2: Update State
    print("\n--- Agent 2: Evolving Context ---")
    run_cmd(
        [
            exe,
            "delete-observations",
            tasks_ent,
            "T-002: Add graph primitives - Status: IN_PROGRESS",
        ]
    )
    run_cmd(
        [
            exe,
            "add-observations",
            tasks_ent,
            "T-002: Add graph primitives - Status: DONE",
        ]
    )
    run_cmd([exe, "compact"])

    # 4. Final Verify
    print("\n--- Final Verification ---")
    res = run_cmd([exe, "query", "T-002"])
    assert "DONE" in res.stdout
    print("Context handoff and restoration verified!")


if __name__ == "__main__":
    main()
