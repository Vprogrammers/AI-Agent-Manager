import sys
import unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"src"))
from codex_profiles import choose_profile, profile_environment

class ProfileTests(unittest.TestCase):
    def test_auto(self):
        self.assertEqual(choose_profile("Summarize this"),"minimal")
        self.assertEqual(choose_profile("Implement a feature"),"coding")
        self.assertEqual(choose_profile("Review GitHub PR"),"full")
    def test_override(self):
        self.assertEqual(choose_profile("Implement code","minimal"),"minimal")
        with self.assertRaises(ValueError):
            choose_profile("x","bad")
    def test_isolation(self):
        import tempfile
        with tempfile.TemporaryDirectory() as base:
            with profile_environment("minimal", {"A": "B", "AAM_PROFILE_HOME": base}) as env:
                self.assertEqual(env["A"], "B")
                directory = Path(env["CODEX_HOME"])
                self.assertTrue(directory.exists())
                self.assertEqual((directory / "config.toml").read_text(), "")
            self.assertTrue(directory.exists())
            with profile_environment("coding", {"AAM_PROFILE_HOME": base}) as env:
                self.assertNotEqual(Path(env["CODEX_HOME"]), directory)
    def test_full_keeps_environment(self):
        with profile_environment("full",{"A":"B"}) as env:
            self.assertEqual(env,{"A":"B"})
if __name__=="__main__":
    unittest.main()
