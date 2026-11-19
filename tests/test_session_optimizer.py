import json
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'src'))
from session_optimizer import after_turn, analytics, cache_policy, rotation_handoff, compact_history, settings, write_checkpoint, context_observation

class OptimizerTests(unittest.TestCase):
    def test_context_is_observation_not_cumulative(self):
        self.assertEqual(context_observation('codex', {'input_tokens': 500}), 500)
        self.assertEqual(context_observation('claude', {'input_tokens': 5, 'cache_read_input_tokens': 50}), 55)
        self.assertIsNone(context_observation('codex', {}))

    def test_checkpoint_does_not_reset_sessions(self):
        with tempfile.TemporaryDirectory() as d:
            state = {'sessions': {'codex:coding': 'abc'}, 'history': [{'tool': 'codex', 'prompt': 'x', 'answer': 'y'}]}
            warning = after_turn(state, 'codex', {'input_tokens': 100}, {'AAM_CHECKPOINT_EVERY_TURNS': '1',
                        'AAM_CONTEXT_WARNING_TOKENS': '50'}, pathlib.Path(d)/'checkpoint.json')
            self.assertTrue(warning)
            self.assertEqual(state['sessions']['codex:coding'], 'abc')
            self.assertTrue((pathlib.Path(d)/'checkpoint.json').exists())

    def test_cache_reuse_and_explicit_override(self):
        state = {'sessions': {'codex:coding': 'abc'}}
        self.assertEqual(cache_policy(state, 'codex', 'coding', 'hello', {}), 'abc')
        self.assertIsNone(cache_policy(state, 'codex', 'coding', 'hello', {'AAM_FORCE_NEW_SESSION': '1'}))

    def test_rotates_oversized_session(self):
        state = {'sessions': {'codex:coding': 'old'}, 'session_input_tokens': {'codex:coding': 707181}}
        self.assertIsNone(cache_policy(state, 'codex', 'coding', 'git status -sb', {}))
        self.assertEqual(cache_policy(state, 'codex', 'coding', 'git status -sb',
                                      {'AAM_AUTO_ROTATE_SESSIONS': '0'}), 'old')
        self.assertEqual(cache_policy(state, 'codex', 'coding', 'git status -sb',
                                      {'AAM_SESSION_ROTATE_TOKENS': '900000'}), 'old')

    def test_rotation_only_carries_context_for_followups(self):
        state = {'history': [{'tool': 'codex', 'prompt': 'analyze budget',
                              'answer': 'Found pricing issue'}]}
        self.assertEqual(rotation_handoff(state, 'codex', 'git status -sb'), 'git status -sb')
        followup = rotation_handoff(state, 'codex', 'a sada proveri')
        self.assertIn('Found pricing issue', followup)
        self.assertIn('a sada proveri', followup)

    def test_bounded_handoff(self):
        state = {'history': [{'prompt': 'x'*1000}]}
        self.assertLessEqual(len(compact_history(state, {'history_chars': 100})), 100)

    def test_analytics(self):
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d)/'usage.jsonl'
            path.write_text(json.dumps({'provider': 'codex', 'profile': 'coding',
                'usage': {'input_tokens': 100, 'cached_input_tokens': 80, 'output_tokens': 10},
                'duration_seconds': 2}) + '\n')
            self.assertEqual(analytics(path)['codex/coding']['cached'], 80)

    def test_invalid_config(self):
        self.assertEqual(settings({'AAM_CHECKPOINT_EVERY_TURNS': 'bad'})['checkpoint_every'], 8)

if __name__ == '__main__':
    unittest.main()
