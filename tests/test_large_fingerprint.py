"""Regression: large dirty worktrees must not disable safe retry verification."""
import tempfile
import subprocess
import unittest
from pathlib import Path
from unittest.mock import patch
from aam_runtime import repository_fingerprint

class FingerprintTests(unittest.TestCase):
    def test_changed_files_over_previous_limit_are_hashed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            subprocess.run(['git', '-C', temp, 'init', '-q'], check=True)
            subprocess.run(['git', '-C', temp, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '--allow-empty', '-qm', 'initial'], check=True)
            (root / 'large.bin').write_bytes(b'a' * 4096)
            with patch('aam_runtime.MAX_FILE_HASH_BYTES', 1), patch('aam_runtime.MAX_FILES', 0):
                before = repository_fingerprint(root)
                (root / 'large.bin').write_bytes(b'b' * 4096)
                after = repository_fingerprint(root)
            self.assertNotEqual(before, after)

if __name__ == '__main__':
    unittest.main()
