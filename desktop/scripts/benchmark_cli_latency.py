#!/usr/bin/env python3
"""Compare CLI latency with and without the macOS sandbox used by AAM.

Run from a Git workspace on macOS:
  python3 desktop/scripts/benchmark_cli_latency.py --provider all --runs 5
  python3 desktop/scripts/benchmark_cli_latency.py --provider opencode --runs 5

Only a read-only prompt is sent. The script never modifies project files.
Results include startup, first output, completion, and sandbox overhead.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import time

PROVIDERS = {
    "opencode": lambda prompt: ["opencode", "run", prompt],
    "copilot": lambda prompt: ["copilot", "-p", prompt, "-s", "--no-ask-user"],
    "antigravity": lambda prompt: ["agy", "--print", prompt],
    "claude": lambda prompt: ["claude", "-p", prompt],
    "codex": lambda prompt: ["codex", "exec", prompt],
}
PROMPT = "Odgovori samo OK. Ne koristi alate i ne menjaj fajlove."


def seatbelt_profile(workspace, provider):
    home = Path.home()
    def q(path):
        return str(path).replace("\\", "\\\\").replace('"', '\\"')
    paths = {
        "opencode": [".config/opencode", ".local/share/opencode", ".local/state/opencode", ".cache/opencode"],
        "copilot": [".copilot", ".config/copilot", ".local/share/copilot", ".cache/copilot"],
        "antigravity": [".config/antigravity", ".local/share/antigravity", ".cache/antigravity", ".gemini"],
    }.get(provider, [])
    writable = [home / ".codex", home / ".local/state/ai-agent-manager", *[home / p for p in paths]]
    git = subprocess.run(["git", "-C", str(workspace), "rev-parse", "--absolute-git-dir"],
                         capture_output=True, text=True)
    if git.returncode:
        raise RuntimeError("Workspace must be a Git repository")
    writable.append(Path(git.stdout.strip()) / "ai-agent-manager")
    rules = " ".join('(subpath "' + q(p) + '")' for p in writable)
    return ('(version 1) (deny default) (allow process*) (allow sysctl-read) '
            '(allow mach-lookup) (allow ipc*) (allow network*) (allow file-read*) '
            '(allow file-write* (subpath "/private/tmp") (subpath "/tmp") '
            '(literal "/dev/null") ' + rules + ' (subpath "' + q(workspace) + '"))')


def measure(command, cwd, timeout):
    started = time.perf_counter()
    try:
        proc = subprocess.Popen(command, cwd=cwd, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    except OSError as exc:
        return {"error": str(exc)}
    spawned = time.perf_counter()
    # communicate() avoids pipe deadlocks; record first stdout byte using a
    # separate reader thread rather than assuming the first output is the end.
    import threading
    first = [None]
    chunks = []
    errors = []
    def drain(stream, target, record_first=False):
        while True:
            block = stream.read(1)
            if not block:
                break
            if record_first and first[0] is None:
                first[0] = time.perf_counter()
            target.append(block)
    out_thread = threading.Thread(target=drain, args=(proc.stdout, chunks, True), daemon=True)
    err_thread = threading.Thread(target=drain, args=(proc.stderr, errors), daemon=True)
    out_thread.start()
    err_thread.start()
    try:
        proc.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
        timed_out = True
    else:
        timed_out = False
    out_thread.join(timeout=2)
    err_thread.join(timeout=2)
    ended = time.perf_counter()
    stderr = b"".join(errors).decode("utf-8", "replace")
    return {
        "spawn_ms": round((spawned - started) * 1000),
        "first_stdout_ms": round((first[0] - started) * 1000) if first[0] else None,
        "total_ms": round((ended - started) * 1000),
        "exit_code": proc.returncode,
        "timeout": timed_out,
        "stdout_bytes": len(chunks),
        # No raw stdout/stderr: agent output could contain private data.
        "stderr_bytes": len(stderr.encode("utf-8")),
    }


def summarize(samples):
    valid = [x["total_ms"] for x in samples if x.get("exit_code") == 0 and not x.get("timeout")]
    if not valid:
        return {"successful_runs": 0}
    return {"successful_runs": len(valid), "median_ms": round(statistics.median(valid)),
            "min_ms": min(valid), "max_ms": max(valid)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", choices=["all", *PROVIDERS], default="all")
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--timeout", type=int, default=90)
    parser.add_argument("--workspace", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path, default=Path("aam-latency-results.json"))
    args = parser.parse_args()
    if args.runs < 1 or args.timeout < 1:
        parser.error("--runs and --timeout must be positive")
    workspace = args.workspace.resolve()
    providers = list(PROVIDERS) if args.provider == "all" else [args.provider]
    report = {"workspace": str(workspace), "runs": args.runs, "providers": {}}
    for provider in providers:
        if not shutil.which(PROVIDERS[provider](PROMPT)[0]):
            print(f"{provider}: CLI unavailable; skipped", flush=True)
            continue
        command = PROVIDERS[provider](PROMPT)
        variants = {"direct": command}
        if sys.platform == "darwin" and shutil.which("sandbox-exec"):
            variants["seatbelt"] = ["sandbox-exec", "-p", seatbelt_profile(workspace, provider), *command]
        results = {}
        # Interleave variants to reduce bias from CLI warmup and network fluctuations.
        for name in variants:
            results[name] = []
        for iteration in range(args.runs):
            for name, cmd in variants.items():
                result = measure(cmd, workspace, args.timeout)
                results[name].append(result)
                print(f"{provider} {name} {iteration + 1}/{args.runs}: {result}", flush=True)
        report["providers"][provider] = {
            "samples": results,
            "summary": {name: summarize(samples) for name, samples in results.items()},
        }
        summary = report["providers"][provider]["summary"]
        if "seatbelt" in summary and summary["direct"].get("successful_runs") and summary["seatbelt"].get("successful_runs"):
            delta = summary["seatbelt"]["median_ms"] - summary["direct"]["median_ms"]
            report["providers"][provider]["sandbox_median_overhead_ms"] = delta
            print(f"{provider}: sandbox median overhead = {delta:+} ms", flush=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Results: {args.output}")


if __name__ == "__main__":
    main()
