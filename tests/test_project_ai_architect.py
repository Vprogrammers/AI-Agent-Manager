import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from project_ai_architect import scan, plan, apply
from smart_router import project_context


class ProjectArchitectTests(unittest.TestCase):
    def test_preview_and_explicit_approval(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".git").mkdir()
            (root / "AGENTS.md").write_text("keep me")
            (root / "package.json").write_text("{}")
            changes = plan(root)
            self.assertEqual(scan(root)["stack"], ["JavaScript/TypeScript"])
            self.assertEqual(len(changes), 3)
            with self.assertRaises(PermissionError):
                apply(root, changes)
            self.assertEqual((root / "AGENTS.md").read_text(), "keep me")
            created = apply(root, changes, approved=True)
            self.assertEqual(len(created), 3)
            self.assertIn(".ai-agent-manager/architect/PROJECT.md", project_context(root))
            self.assertEqual(apply(root, plan(root), approved=True), [])

    def test_preserves_existing_managed_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".git").mkdir()
            target = root / ".ai-agent-manager/architect/PROJECT.md"
            target.parent.mkdir(parents=True)
            target.write_text("custom instructions")
            changes = plan(root)
            self.assertTrue(any(c["status"] == "conflict" for c in changes))
            apply(root, changes, approved=True)
            self.assertEqual(target.read_text(), "custom instructions")


if __name__ == "__main__":
    unittest.main()
