#!/usr/bin/env python3
"""Measure observable latency phases for every installed AAM CLI provider.

Default: no model calls. --include-model explicitly sends read-only requests.
This cannot inspect private CLI internals (DNS, auth, model inference); those
are grouped as 'CLI internal / waiting for first byte'.

Example:
 python3 desktop/scripts/trace_agent_phases.py --workspace . --runs 3 --include-model
"""
import argparse
import json
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import time

from benchmark_cli_latency import PROVIDERS, PROMPT, measure, seatbelt_profile
from diagnose_cli_latency import VERSIONS


def median(values):
    return round(statistics.median(values)) if values else None


def summarize(samples):
    good = [s for s in samples if s.get("exit_code") == 0 and not s.get("timeout")]
    phases = {}
    for s in good:
        spawn = s.get("spawn_ms")
        first = s.get("first_stdout_ms")
        total = s.get("total_ms")
        for key, value in {
            "spawn_ms": spawn,
            "cli_to_first_stdout_ms": max(0, first - spawn) if first is not None and spawn is not None else None,
            "first_stdout_to_exit_ms": max(0, total - first) if first is not None and total is not None else None,
            "total_ms": total,
        }.items():
            if value is not None:
                phases.setdefault(key, []).append(value)
    return {"successful": len(good), "attempts": len(samples),
            "phases_median_ms": {k: median(v) for k, v in phases.items()},
            "timeouts": sum(bool(s.get("timeout")) for s in samples),
            "failed": sum(s.get("exit_code") not in (0, None) for s in samples)}


def timed_git(workspace):
    started = time.perf_counter()
    try:
        proc = subprocess.run(["git", "-C", str(workspace), "status", "--porcelain"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
        status = proc.returncode
    except subprocess.TimeoutExpired:
        status = "timeout"
    return {"elapsed_ms": round((time.perf_counter() - started) * 1000), "exit_code": status}


def run(workspace, runs, timeout, include_model, selected):
    report = {"workspace": str(workspace), "model_requests_enabled": include_model,
              "runs": runs, "git": [timed_git(workspace) for _ in range(runs)], "providers": {}}
    for provider in selected:
        exe = PROVIDERS[provider](PROMPT)[0]
        if not shutil.which(exe):
            report["providers"][provider] = {"available": False, "reason": "CLI missing"}
            print(f"{provider}: missing; skipped", flush=True)
            continue
        phases = {"version_direct": VERSIONS[provider]}
        profile = None
        if sys.platform == "darwin" and shutil.which("sandbox-exec"):
            profile = seatbelt_profile(workspace, provider)
            phases["version_sandbox"] = ["sandbox-exec", "-p", profile, *VERSIONS[provider]]
        if include_model:
            phases["model_direct"] = PROVIDERS[provider](PROMPT)
            if profile:
                phases["model_sandbox"] = ["sandbox-exec", "-p", profile, *PROVIDERS[provider](PROMPT)]
        samples = {name: [] for name in phases}
        # Interleave variants to reduce bias from network and provider warmup.
        for i in range(runs):
            for name, command in phases.items():
                sample = measure(command, workspace, timeout)
                samples[name].append(sample)
                print(f"{provider} {name} {i+1}/{runs}: {sample}", flush=True)
        report["providers"][provider] = {
            "available": True,
            "variants": {name: {"summary": summarize(data), "samples": data}
                         for name, data in samples.items()},
        }
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--workspace", type=Path, default=Path.cwd())
    p.add_argument("--provider", choices=["all", *PROVIDERS], default="all")
    p.add_argument("--runs", type=int, default=3)
    p.add_argument("--timeout", type=int, default=30)
    p.add_argument("--include-model", action="store_true")
    p.add_argument("--output", type=Path, default=Path("/tmp/aam-agent-phase-trace.json"))
    a = p.parse_args()
    if a.runs < 1 or a.timeout < 1:
        p.error("runs and timeout must be positive")
    workspace = a.workspace.expanduser().resolve(strict=True)
    if not workspace.is_dir():
        p.error("workspace must be a directory")
    selected = list(PROVIDERS) if a.provider == "all" else [a.provider]
    report = run(workspace, a.runs, a.timeout, a.include_model, selected)
    a.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print("\nMedian phase breakdown (ms):", flush=True)
    for provider, entry in report["providers"].items():
        print(provider, flush=True)
        for name, data in entry.get("variants", {}).items():
            print(f"  {name}: {data['summary']}", flush=True)
    print(f"Results: {a.output}", flush=True)


if __name__ == "__main__":
    main()
