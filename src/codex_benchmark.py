#!/usr/bin/env python3
"""Read-only Codex profile benchmark. Reports provider token usage, not monetary cost."""
import argparse
import json
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path
from codex_profiles import profile_environment

PROMPT = "Reply with exactly OK. Do not use tools."

def measure(codex, model, profile, runs):
    results = []
    for i in range(runs):
        with tempfile.TemporaryDirectory(prefix="aam-benchmark-") as cwd:
            with profile_environment(profile) as env:
                cmd = [codex, "exec", "--skip-git-repo-check", "--model", model, "--json", PROMPT]
                start = time.monotonic()
                proc = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, timeout=180)
                duration = round(time.monotonic()-start, 3)
                usage = None
                for line in proc.stdout.splitlines():
                    try:
                        event = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if event.get("type") == "turn.completed":
                        usage = event.get("usage")
                results.append({"run":i+1, "exit_code":proc.returncode,
                                "duration_seconds":duration, "usage":usage,
                                "error":proc.stderr[-300:] if proc.returncode else None})
    return results

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--profiles", nargs="+", choices=("default","minimal","coding","full"), default=["default","minimal"])
    parser.add_argument("--output", default="aam-benchmark.json")
    args = parser.parse_args()
    if not 1 <= args.runs <= 20:
        parser.error("--runs must be 1..20")
    codex = shutil.which("codex")
    if not codex:
        parser.error("codex executable not found")
    data = {"model":args.model, "prompt":PROMPT, "profiles":{}}
    for profile in args.profiles:
        runs = measure(codex,args.model,profile,args.runs)
        valid = [r for r in runs if r["exit_code"] == 0 and isinstance(r["usage"],dict)]
        inputs = [r["usage"]["input_tokens"] for r in valid if isinstance(r["usage"].get("input_tokens"),int)]
        durations = [r["duration_seconds"] for r in valid]
        data["profiles"][profile] = {"runs":runs, "median_input_tokens":statistics.median(inputs) if inputs else None,
                                      "median_duration_seconds":statistics.median(durations) if durations else None}
        print(profile, "successful:",len(valid),"/",len(runs),"median input:",data["profiles"][profile]["median_input_tokens"])
    Path(args.output).write_text(json.dumps(data,indent=2)+"\n")
    print("Saved:",args.output)

if __name__=="__main__":
    main()
