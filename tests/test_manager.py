import importlib.util
import pathlib
import unittest

PATH = pathlib.Path(__file__).resolve().parents[1] / 'src' / 'ai_agent_manager.py'
import sys
sys.path.insert(0, str(PATH.parent))
spec = importlib.util.spec_from_file_location('ai_agent_manager', PATH)
manager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manager)

class ManagerTests(unittest.TestCase):
    def test_terminal_clear_uses_escape_sequence(self):
        from unittest.mock import patch
        import io
        output = io.StringIO()
        with patch.object(output, 'isatty', return_value=True), \
             patch.object(manager.sys, 'stdout', output):
            manager.show_agent_ready('claude', pathlib.Path('.'), clear=True)
        self.assertTrue(output.getvalue().startswith(chr(27) + '[2J' + chr(27) + '[H'))
        self.assertIn('CLAUDE READY', output.getvalue())
    def test_model_routing(self):
        self.assertEqual(manager.model_for('codex', 'fix typo', {'AAM_CODEX_LOW_MODEL': 'small'}), 'small')
        self.assertEqual(manager.model_for('claude', 'security migration', {'AAM_CLAUDE_HIGH_MODEL': 'large'}), 'large')
    def test_default_model(self):
        self.assertEqual(manager.model_for('codex', 'implement UI', {}), '')
    def test_codex_command(self):
        self.assertEqual(manager.build_command('codex', 'm', None, 'hello'), ['codex','exec','--model','m','--json','hello'])
    def test_claude_command(self):
        self.assertEqual(manager.build_command('claude', '', 'session', 'hello'), ['claude','-p','--output-format','json','--resume','session','hello'])
    def test_codex_result(self):
        sid, msg = manager.parse_result('codex', '{"type":"thread.started","thread_id":"abc"}\n{"type":"item.completed","item":{"type":"agent_message","text":"OK"}}')
        self.assertEqual((sid,msg), ('abc','OK'))
    def test_claude_result(self):
        self.assertEqual(manager.parse_result('claude', '{"session_id":"x","result":"OK"}'), ('x','OK'))

if __name__ == '__main__':
    unittest.main()
