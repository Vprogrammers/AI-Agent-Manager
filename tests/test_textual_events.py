import pathlib
import sys
import unittest
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'src'))
from aam_tui import event_label, token_summary

class AgentEventTests(unittest.TestCase):
    def test_codex_step_redacts_command(self):
        self.assertEqual(event_label({'type':'item.started','item':{'type':'command_execution','command':'SECRET'}}), 'Command · started')
    def test_token_values(self):
        self.assertIn('cached 2,000', token_summary({'input_tokens':4000,'cached_input_tokens':2000}))
    def test_claude_tool_step(self):
        self.assertEqual(event_label({'type':'content_block_start','content_block':{'type':'tool_use','name':'Bash'}}), 'Tool · Bash')

if __name__ == '__main__':
    unittest.main()
