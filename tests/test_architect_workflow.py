"""End-to-end Architect workflow: initial generation, migration, updates, conflicts."""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SRC = Path(__file__).resolve().parents[1] / "src"
sys.path.insert(0, str(SRC))
from project_ai_architect import plan, apply, proposals
from project_architect_refresh import refresh_plan, refresh_apply, _legacy_generated, STATE


class ArchitectWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / ".git").mkdir()
        (self.root / "AGENTS.md").write_text("User instructions stay untouched\n")
        (self.root / "main.py").write_text("class First:\n    pass\n")

    def test_full_cli_flow(self):
        preview = subprocess.run([sys.executable, str(SRC / "project_ai_architect.py"),
                                  "--root", str(self.root), "--json"], capture_output=True, text=True)
        self.assertEqual(preview.returncode, 0, preview.stderr)
        self.assertFalse((self.root / STATE).exists())
        changes = plan(self.root)
        with self.assertRaises(PermissionError):
            apply(self.root, changes)
        self.assertEqual(len(apply(self.root, changes, approved=True)), 3)
        self.assertTrue((self.root / STATE).is_file())
        self.assertTrue(all(x["status"] == "unchanged" for x in plan(self.root)))
        (self.root / "main.py").write_text("class Second:\n    pass\n")
        self.assertEqual(len([x for x in plan(self.root) if x["status"] == "update"]), 1)
        self.assertEqual(len(apply(self.root, plan(self.root), approved=True)), 1)
        self.assertIn("main.py: Second", (self.root / ".ai-agent-manager/architect/PROJECT.md").read_text())
        self.assertEqual((self.root / "AGENTS.md").read_text(), "User instructions stay untouched\n")

    def test_migrate_legacy_only_if_unchanged(self):
        legacy = _legacy_generated(self.root)
        for relative, value in legacy.items():
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(value)
        self.assertTrue(all(x["status"] == "update" for x in refresh_plan(self.root)))
        target = self.root / ".ai-agent-manager/architect/PROJECT.md"
        target.write_text("customized")
        changes = refresh_plan(self.root)
        self.assertEqual(next(x for x in changes if x["path"] == ".ai-agent-manager/architect/PROJECT.md")["status"], "conflict")
        self.assertEqual(len(refresh_apply(self.root, changes, approved=True)), 2)
        self.assertEqual(target.read_text(), "customized")

    def test_symlink_manifest_blocks_writes(self):
        external = self.root / "external"
        external.write_text("keep")
        manifest = self.root / STATE
        manifest.parent.mkdir(parents=True)
        try:
            manifest.symlink_to(external)
        except (NotImplementedError, OSError):
            self.skipTest("symlinks unavailable")
        with self.assertRaises(ValueError):
            refresh_apply(self.root, refresh_plan(self.root), approved=True)
        self.assertEqual(external.read_text(), "keep")
        self.assertFalse((self.root / ".ai-agent-manager/architect/PROJECT.md").exists())


if __name__ == "__main__":
    unittest.main()
