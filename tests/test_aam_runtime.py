import json
import pathlib
import sys
import tempfile
import unittest
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'src'))
from aam_runtime import handoff_delta, model_for_session, repository_fingerprint
from session_optimizer import load_checkpoint, write_checkpoint, settings

class DesktopStreamingTests(unittest.TestCase):
    def test_claude_desktop_command_uses_partial_streaming(self):
        import os
        from unittest.mock import patch
        from ai_agent_manager import build_command
        with patch.dict(os.environ, {"AAM_DESKTOP_STREAM": "1", "AAM_PERMISSION_MODE": "default"}):
            command = build_command("claude", None, None, "Hello")
        self.assertIn("stream-json", command)
        self.assertIn("--include-partial-messages", command)

    def test_claude_terminal_command_keeps_legacy_json(self):
        import os
        from unittest.mock import patch
        from ai_agent_manager import build_command
        with patch.dict(os.environ, {"AAM_DESKTOP_STREAM": "0", "AAM_PERMISSION_MODE": "default"}):
            command = build_command("claude", None, None, "Hello")
        self.assertEqual(command[command.index("--output-format") + 1], "json")


class RuntimeOptimizationTests(unittest.TestCase):
    def test_handoff_to_resumed_provider(self):
        state={'history':[{'tool':'claude','prompt':'old'},{'tool':'codex','prompt':'new'}]}
        self.assertIn('new', handoff_delta(state,'claude',{'history_chars':4000}))
        self.assertNotIn('old', handoff_delta(state,'claude',{'history_chars':4000}))
    def test_model_pin(self):
        self.assertEqual(model_for_session({'session_models':{'claude':'old'}},'claude','sid','new'),'old')
    def test_checkpoint_roundtrip(self):
        with tempfile.TemporaryDirectory() as d:
            p=pathlib.Path(d)/'checkpoint.json'
            write_checkpoint(p,{'sessions':{'claude':'sid'},'last_completed':{'tool':'claude'}},settings({}))
            self.assertEqual(load_checkpoint(p)['sessions']['claude'],'sid')
    def test_missing_checkpoint(self):
        self.assertIsNone(load_checkpoint('/path/that/does/not/exist'))
if __name__ == '__main__':
    unittest.main()
