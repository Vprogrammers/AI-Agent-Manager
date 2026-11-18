import sys
import pathlib
import unittest
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'src'))
from aam_tui import event_label, token_summary


class TuiPresentationTests(unittest.TestCase):
    def test_safe_event_labels(self):
        event = {'type': 'item.started', 'item': {'type': 'command_execution',
                 'command': 'echo SECRET'}}
        self.assertEqual(event_label(event), 'Command · started')
        self.assertNotIn('SECRET', event_label(event))

    def test_completion(self):
        self.assertEqual(event_label({'type': 'turn.completed'}), 'Turn completed')

    def test_unknown_event(self):
        self.assertIsNone(event_label({'type': 'thread.started'}))

    def test_usage_only_reported(self):
        self.assertEqual(token_summary({}), 'Tokens: not reported')
        self.assertEqual(token_summary({'input_tokens': 100, 'output_tokens': 20}),
                         'Tokens: input 100 · output 20')


if __name__ == '__main__':
    unittest.main()
