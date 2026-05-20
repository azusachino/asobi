import subprocess
import json
import os
from pathlib import Path

def run_cmd(args):
    print(f"Running: {' '.join(args)}")
    return subprocess.run(args, capture_output=True, text=True)

def main():
    # 1. Setup project-local env
    os.makedirs(".rosemary/topics", exist_ok=True)
    
    # Ensure debug binary exists
    print("Building debug binary...")
    subprocess.run(["cargo", "build"], check=True)
    exe = "./target/debug/rosemary"

    # 2. Test Ingest
    print("\n--- Testing Ingest ---")
    res = run_cmd([exe, "ingest", "examples"])
    assert res.returncode == 0, res.stderr
    print("Ingest OK")

    # 3. Test Query
    print("\n--- Testing Query ---")
    res = run_cmd([exe, "query", "tokio"])
    assert res.returncode == 0, res.stderr
    assert "tokio" in res.stdout.lower() or "no results" in res.stdout.lower()
    print("Query OK")

    # 4. Test MCP (read_graph)
    print("\n--- Testing MCP ---")
    req = {"jsonrpc": "2.0", "id": 1, "method": "read_graph"}
    process = subprocess.Popen([exe, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    stdout, stderr = process.communicate(input=json.dumps(req))
    
    assert process.returncode == 0, stderr
    resp = json.loads(stdout)
    assert "result" in resp, f"Unexpected MCP response: {resp}"
    print("MCP OK")

    # 5. Test Compact
    print("\n--- Testing Compact ---")
    res = run_cmd([exe, "compact"])
    assert res.returncode == 0, res.stderr
    print("Compact OK")

    print("\n✅ All CLI subcommands verified successfully!")

if __name__ == "__main__":
    main()
