import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch
from subprocess import CompletedProcess

FILE = pathlib.Path(__file__).resolve().parents[1] / 'src' / 'ai_agent_manager.py'
spec = importlib.util.spec_from_file_location('manager', FILE)
manager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manager)

class BridgeTests(unittest.TestCase):
    def test_codex_usage(self):
        value = manager.usage_from_output('codex', json.dumps({'type':'turn.completed','usage':{'input_tokens':100,'cached_input_tokens':80,'output_tokens':3}}))
        self.assertEqual(value['cached_input_tokens'], 80)
    def test_missing_usage(self):
        self.assertEqual(manager.usage_from_output('codex', '{}'), {})
    def test_claude_usage(self):
        self.assertEqual(manager.usage_from_output('claude', json.dumps({'usage':{'input_tokens':7,'output_tokens':2}})), {'input_tokens':7,'output_tokens':2})
    def test_handoff_is_bounded(self):
        with tempfile.TemporaryDirectory() as d:
            with patch.object(manager, 'git', return_value='test'):
                msg = manager.handoff(pathlib.Path(d), {'history':[{'prompt':'x'*30000}]})
                self.assertLess(len(msg), 8500)
    def test_failed_turn_does_not_update_session(self):
        with tempfile.TemporaryDirectory() as d:
            with patch.object(manager, 'store_path', return_value=pathlib.Path(d)/'state.json'):
                state = {'sessions':{'codex':'original'}}
                def runner(*args, **kwargs):
                    return CompletedProcess(args[0], 1, '', 'rate limit')
                with self.assertRaises(RuntimeError):
                    manager.run_turn(pathlib.Path(d), 'codex', 'test', state, {}, runner)
                self.assertEqual(state['sessions']['codex'], 'original')
    def test_usage_log(self):
        with tempfile.TemporaryDirectory() as d:
            with patch.object(manager, 'store_path', return_value=pathlib.Path(d)/'state.json'):
                manager.record_usage(pathlib.Path(d), 'codex', 'm', 1.5, {}, 'ok')
                entry=json.loads((pathlib.Path(d)/'usage.jsonl').read_text().strip())
                self.assertEqual(entry['duration_seconds'], 1.5)

if __name__ == '__main__':
    unittest.main()
