import unittest
from smart_router import classify, route

class SmartRouterTests(unittest.TestCase):
    def test_coding_prefers_codex(self):
        self.assertEqual(route("implement Kotlin tests", {}, {}, {"claude","codex"})["provider"], "codex")
    def test_analysis_prefers_claude(self):
        self.assertEqual(route("security architecture review", {}, {}, {"claude","codex"})["provider"], "claude")
    def test_reuse_session(self):
        d=route("fix bug", {"last_tool":"claude","sessions":{"claude":"abc"}}, {}, {"claude","codex"})
        self.assertEqual(d["provider"], "claude")
    def test_limit_override(self):
        d=route("security architecture", {"last_tool":"claude","pending_switch":"codex"}, {}, {"claude","codex"})
        self.assertEqual(d["provider"], "codex")
    def test_missing_provider(self):
        self.assertEqual(route("architecture", {}, {}, {"codex"})["provider"], "codex")
    def test_no_extra_model_call(self):
        self.assertEqual(classify("README typo"), "docs")
        self.assertEqual(route("README typo", {}, {}, {"codex"})["model"], "CLI default / existing session")
    def test_none_available(self):
        with self.assertRaises(ValueError):
            route("test", {}, {}, set())
if __name__ == "__main__":
    unittest.main()
