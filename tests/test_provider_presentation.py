import pathlib
import sys
import unittest
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'src'))
from aam_tui import provider_model, provider_usage, step_detail

class ProviderPresentationTests(unittest.TestCase):
    def test_model_from_claude_message(self):
        self.assertEqual(provider_model({'type':'message_start','message':{'model':'claude-test'}}), 'claude-test')
    def test_model_not_inferred(self):
        self.assertIsNone(provider_model({'type':'turn.started'}))
    def test_usage_from_message(self):
        self.assertEqual(provider_usage({'message':{'usage':{'input_tokens':12,'output_tokens':3}}}), {'input_tokens':12,'output_tokens':3})
    def test_usage_ignores_invalid(self):
        self.assertEqual(provider_usage({'usage':{'input_tokens':'12','output_tokens':-1}}), {})
    def test_no_command_leak(self):
        self.assertNotIn('SECRET', step_detail({'type':'item.started','item':{'type':'command_execution','command':'SECRET'}}))
if __name__ == '__main__':
    unittest.main()
