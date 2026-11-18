"""Explicit read-only update check against the configured Git upstream."""
import subprocess
from pathlib import Path

def check_updates(root=None):
    checkout = Path(__file__).resolve().parents[1]
    def git(*args, timeout=12):
        return subprocess.run(['git', '-C', str(checkout), *args],
                              capture_output=True, text=True, timeout=timeout)
    try:
        upstream = git('rev-parse', '--abbrev-ref', '--symbolic-full-name', '@{upstream}')
        if upstream.returncode:
            return 'No configured upstream branch.'
        remote = git('config', '--get', 'branch.' + git('branch', '--show-current').stdout.strip() + '.remote')
        merge = git('rev-parse', '--symbolic-full-name', '@{upstream}')
        if remote.returncode or not remote.stdout.strip():
            return 'No configured upstream remote.'
        remote_name = remote.stdout.strip()
        remote_ref = git('config', '--get', 'branch.' + git('branch', '--show-current').stdout.strip() + '.merge')
        if remote_ref.returncode or not remote_ref.stdout.strip():
            return 'No configured upstream branch ref.'
        live = git('ls-remote', '--exit-code', remote_name, remote_ref.stdout.strip(), timeout=15)
        if live.returncode:
            return 'Remote update check unavailable (network or authentication).'
        remote_sha = live.stdout.split()[0]
        local = git('rev-parse', 'HEAD')
        if local.returncode:
            return 'Cannot resolve local HEAD.'
        if remote_sha == local.stdout.strip():
            return 'Up to date with remote upstream.'
        tracked = git('rev-parse', '@{upstream}')
        if tracked.returncode or tracked.stdout.strip() != remote_sha:
            return 'Remote upstream has changed. Run git fetch then git status -sb; update with git pull --ff-only when safe.'
        counts = git('rev-list', '--left-right', '--count', 'HEAD...@{upstream}')
        if counts.returncode:
            return 'Remote differs; inspect git status -sb.'
        ahead, behind = map(int, counts.stdout.split())
        return f'Local commits ahead: {ahead}; behind: {behind}. Use git pull --ff-only when safe.'
    except (OSError, subprocess.SubprocessError, ValueError, IndexError):
        return 'Update check unavailable; inspect Git and network configuration.'
