"""Explicit, read-only upstream update check for the installed AAM checkout."""
import subprocess
from pathlib import Path

def check_updates(root=None):
    checkout = Path(__file__).resolve().parents[1]
    def git(*args):
        return subprocess.run(['git', '-C', str(checkout), *args], capture_output=True,
                              text=True, timeout=12)
    try:
        branch = git('rev-parse', '--abbrev-ref', '--symbolic-full-name', '@{upstream}')
        if branch.returncode:
            return 'Update check unavailable: no configured upstream branch.'
        fetched = git('fetch', '--quiet', '--no-tags', '--dry-run')
        # Fetch --dry-run does not update refs: report local tracking information only.
        counts = git('rev-list', '--left-right', '--count', 'HEAD...@{upstream}')
        if counts.returncode:
            return 'Update check unavailable: cannot compare upstream tracking ref.'
        ahead, behind = map(int, counts.stdout.split())
        if behind:
            return f'{behind} upstream commit(s) behind locally tracked branch. Run git fetch then git pull --ff-only to verify and update.'
        if ahead:
            return f'{ahead} local commit(s) ahead of tracked upstream.'
        return 'Up to date against locally tracked upstream. Run git fetch for a fresh remote comparison.'
    except (OSError, subprocess.SubprocessError, ValueError):
        return 'Update check unavailable; verify Git and network access.'
