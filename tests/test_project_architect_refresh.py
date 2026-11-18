import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from project_architect_refresh import analyze, refresh_plan, refresh_apply


class ArchitectRefreshTests(unittest.TestCase):
    def test_refresh_and_user_edit_conflict(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".git").mkdir()
            (root / "main.py").write_text("class Example:\n    pass\n")
            self.assertIn("main.py: Example", analyze(root)["symbols"])
            preview = refresh_plan(root)
            self.assertTrue(all(c["status"] == "create" for c in preview))
            with self.assertRaises(PermissionError):
                refresh_apply(root, preview)
            self.assertEqual(len(refresh_apply(root, preview, approved=True)), 3)
            self.assertTrue(all(c["status"] == "unchanged" for c in refresh_plan(root)))
            (root / "main.py").write_text("class Changed:\n    pass\n")
            self.assertTrue(any(c["status"] == "update" for c in refresh_plan(root)))
            target = root / ".ai-agent-manager/architect/PROJECT.md"
            target.write_text("user-owned changes")
            changes = refresh_plan(root)
            self.assertTrue(any(c["status"] == "conflict" for c in changes))
            refresh_apply(root, changes, approved=True)
            self.assertEqual(target.read_text(), "user-owned changes")

    def test_stale_preview_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".git").mkdir()
            preview = refresh_plan(root)
            target = root / preview[0]["path"]
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("concurrent change")
            with self.assertRaises(RuntimeError):
                refresh_apply(root, preview, approved=True)


if __name__ == "__main__":
    unittest.main()
