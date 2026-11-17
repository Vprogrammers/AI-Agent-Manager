import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch
from subprocess import CompletedProcess

FILE = pathlib.Path(__file__).resolve().parents[1] / 'src' / 'ai_agent_manager.py'
import sys
sys.path.insert(0, str(FILE.parent))
spec = importlib.util.spec_from_file_location('manager', FILE)
manager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manager)

class BridgeTests(unittest.TestCase):
    def test_claude_limit_is_compact_and_schedules_failover(self):
        with tempfile.TemporaryDirectory() as d:
            with patch.object(manager, 'store_path', return_value=pathlib.Path(d)/'state.json'):
                payload = json.dumps({'is_error': True, 'result': 'Usage limit reached; resets Nov 19 at 10am (Europe/Belgrade)', 'api_error': 'usage_limit_reached', 'private': 'SECRET' * 1000})
                def runner(cmd, **kwargs):
                    return CompletedProcess(cmd, 1, payload, '')
                with self.assertRaisesRegex(RuntimeError, 'Claude \\| LIMIT REACHED') as ctx:
                    manager.run_turn(pathlib.Path(d), 'claude', 'test', {'sessions': {}}, {}, runner)
                self.assertNotIn('SECRET', str(ctx.exception))
                self.assertLess(len(str(ctx.exception)), 160)

    def test_nonlimit_failure_remains_error(self):
        self.assertIsNone(manager.limit_summary('claude', '{"error":"bad request"}', ''))

    def test_safe_claude_rejection_requires_zero_work(self):
        base = {'is_error': True, 'num_turns': 0, 'duration_api_ms': 0,
                'total_cost_usd': 0, 'usage': {'input_tokens': 0, 'output_tokens': 0}}
        self.assertTrue(manager.safe_claude_rejection(json.dumps(base)))
        for change in ({'num_turns': 1}, {'duration_api_ms': 5},
                       {'total_cost_usd': 0.1}, {'usage': {'input_tokens': 1}},
                       {'usage': {}}, {'is_error': False}):
            self.assertFalse(manager.safe_claude_rejection(json.dumps({**base, **change})))
        self.assertFalse(manager.safe_claude_rejection('not json'))

    def test_safe_limit_raises_retryable_error(self):
        with tempfile.TemporaryDirectory() as d:
            with patch.object(manager, 'store_path', return_value=pathlib.Path(d)/'state.json'), \\
                 patch.object(manager, 'repository_snapshot', return_value=('head', 'clean')):
                payload = json.dumps({'is_error': True, 'result': 'Usage limit reached',
                                      'num_turns': 0, 'duration_api_ms': 0,
                                      'total_cost_usd': 0, 'usage': {'input_tokens': 0}})
                def runner(cmd, **kwargs):
                    return CompletedProcess(cmd, 1, payload, '')
                with self.assertRaises(manager.ProviderLimitError) as ctx:
                    manager.run_turn(pathlib.Path(d), 'claude', 'read md', {}, {}, runner)
                self.assertTrue(ctx.exception.safe_retry)

    def test_changed_repository_blocks_retry(self):
        with tempfile.TemporaryDirectory() as d:
            with patch.object(manager, 'store_path', return_value=pathlib.Path(d)/'state.json'), \\
                 patch.object(manager, 'repository_snapshot', side_effect=[('head', 'before'), ('head', 'after')]):
                payload = json.dumps({'is_error': True, 'result': 'Usage limit reached',
                                      'num_turns': 0, 'duration_api_ms': 0,
                                      'total_cost_usd': 0, 'usage': {'input_tokens': 0}})
                def runner(cmd, **kwargs):
                    return CompletedProcess(cmd, 1, payload, '')
                with self.assertRaises(manager.ProviderLimitError) as ctx:
                    manager.run_turn(pathlib.Path(d), 'claude', 'read md', {}, {}, runner)
                self.assertFalse(ctx.exception.safe_retry)

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

    def test_profile_home_persists(self):
        import sys
        sys.path.insert(0, str(FILE.parent))
        from codex_profiles import profile_environment
        with tempfile.TemporaryDirectory() as d:
            env = {'AAM_PROFILE_HOME': d}
            with profile_environment('coding', env) as first:
                home = pathlib.Path(first['CODEX_HOME'])
                (home / 'session-marker').write_text('persisted')
            with profile_environment('coding', env) as second:
                self.assertEqual(second['CODEX_HOME'], str(home))
                self.assertEqual((home / 'session-marker').read_text(), 'persisted')
            with profile_environment('minimal', env) as third:
                self.assertNotEqual(third['CODEX_HOME'], str(home))
    def test_codex_session_scoped_by_profile(self):
        with tempfile.TemporaryDirectory() as d:
            with patch.object(manager, 'store_path', return_value=pathlib.Path(d)/'state.json'):
                state = {'sessions': {'codex:coding': 'existing'}}
                output = '\n'.join([
                    json.dumps({'type': 'thread.started', 'thread_id': 'existing'}),
                    json.dumps({'type': 'item.completed', 'item': {'type': 'agent_message', 'text': 'OK'}}),
                ])
                def runner(cmd, **kwargs):
                    self.assertEqual(cmd[2:4], ['resume', 'existing'])
                    return CompletedProcess(cmd, 0, output, '')
                manager.run_turn(pathlib.Path(d), 'codex', 'fix code', state, {}, runner)
                self.assertEqual(state['sessions']['codex:coding'], 'existing')

if __name__ == '__main__':
    unittest.main()
