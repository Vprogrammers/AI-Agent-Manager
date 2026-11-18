import pathlib
import sys
import unittest
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'src'))
from aam_tui import streamed_answer

class StreamingTests(unittest.TestCase):
    def test_codex_snapshot(self):
        self.assertEqual(streamed_answer({'type':'item.updated','item':{'type':'agent_message','text':'hello'}}), 'hello')
    def test_no_command_output(self):
        self.assertIsNone(streamed_answer({'type':'item.completed','item':{'type':'command_execution','text':'SECRET'}}))
    def test_claude_assistant(self):
        self.assertEqual(streamed_answer({'type':'assistant','message':{'content':[{'type':'text','text':'hello'}]}}), 'hello')
    def test_no_thinking(self):
        self.assertIsNone(streamed_answer({'type':'assistant','message':{'content':[{'type':'thinking','thinking':'SECRET'}]}}))
if __name__ == '__main__':
    unittest.main()
