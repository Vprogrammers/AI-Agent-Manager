#!/usr/bin/env python3
"""Diagnose CLI latency without confusing model time with AAM overhead.

Run: python3 desktop/scripts/diagnose_cli_latency.py --workspace /path/to/repo
Default diagnostics do NOT send AI prompts. Add --include-model to measure model requests.
No output content, environment variables or credentials are recorded.
"""
import argparse
import json
from pathlib import Path
import shutil
import statistics
import subprocess
import sys

from benchmark_cli_latency import PROVIDERS, PROMPT, measure, seatbelt_profile

PROBES = {
    "python": [sys.executable, "-c", "print('OK')"],
    "git": ["git", "status", "--porcelain"],
}
VERSIONS = {
    "opencode": ["opencode", "--version"],
    "copilot": ["copilot", "--version"],
    "antigravity": ["agy", "--version"],
    "claude": ["claude", "--version"],
    "codex": ["codex", "--version"],
}


def phase_stats(samples):
    valid = [s for s in samples if s.get("exit_code") == 0 and not s.get("timeout")]
    result = {"successful_runs": len(valid), "attempts": len(samples)}
    for key in ("spawn_ms", "first_stdout_ms", "total_ms"):
        values = [s[key] for s in valid if s.get(key) is not None]
        result[key] = round(statistics.median(values)) if values else None
    return result


def run_probe(command, workspace, runs, timeout, sandbox=None):
    samples = []
    for _ in range(runs):
        cmd = (["sandbox-exec", "-p", sandbox] if sandbox else []) + command
        samples.append(measure(cmd, workspace, timeout))
    return {"summary": phase_stats(samples), "samples": samples}


def diagnose(workspace, runs, timeout, include_model=False, providers=None):
    providers = providers or list(PROVIDERS)
    result = {"workspace": str(workspace), "runs": runs, "model_requests_enabled": include_model,
              "probes": {}, "providers": {}}
    for name, command in PROBES.items():
        result["probes"][name] = run_probe(command, workspace, runs, timeout)
    for provider in providers:
        if not shutil.which(VERSIONS[provider][0]):
            result["providers"][provider] = {"available": False}
            continue
        entry = {"available": True}
        entry["version"] = run_probe(VERSIONS[provider], workspace, runs, timeout)
        if sys.platform == "darwin" and shutil.which("sandbox-exec"):
            profile = seatbelt_profile(workspace, provider)
            entry["version_sandbox"] = run_probe(VERSIONS[provider], workspace, runs, timeout, profile)
            entry["sandbox_baseline"] = run_probe(PROBES["python"], workspace, runs, timeout, profile)
        if include_model:
            command = PROVIDERS[provider](PROMPT)
            entry["model_direct"] = run_probe(command, workspace, runs, timeout)
            if sys.platform == "darwin" and shutil.which("sandbox-exec"):
                entry["model_sandbox"] = run_probe(command, workspace, runs, timeout, profile)
        result["providers"][provider] = entry
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, default=Path.cwd())
    parser.add_argument("--provider", choices=["all", *PROVIDERS], default="all")
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--timeout", type=int, default=90)
    parser.add_argument("--include-model", action="store_true", help="Send read-only AI requests (may incur cost)")
    parser.add_argument("--output", type=Path, default=Path("/tmp/aam-phase-diagnostics.json"))
    args = parser.parse_args()
    if args.runs < 1 or args.timeout < 1:
        parser.error("--runs and --timeout must be positive")
    workspace = args.workspace.expanduser().resolve(strict=True)
    if not workspace.is_dir():
        parser.error("workspace must be a directory")
    selected = list(PROVIDERS) if args.provider == "all" else [args.provider]
    result = diagnose(workspace, args.runs, args.timeout, args.include_model, selected)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    for provider, entry in result["providers"].items():
        print(provider)
        for name, probe in entry.items():
            if isinstance(probe, dict) and "summary" in probe:
                print(f"  {name}: {probe['summary']}")
    print(f"Results: {args.output}")


if __name__ == "__main__":
    main()
