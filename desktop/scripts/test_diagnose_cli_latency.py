import sys
from pathlib import Path
from unittest import TestCase, mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import diagnose_cli_latency as diagnostic


class PhaseDiagnosticTests(TestCase):
    def test_phase_stats_uses_successful_samples_only(self):
        samples = [
            {"exit_code": 0, "timeout": False, "spawn_ms": 2, "first_stdout_ms": 100, "total_ms": 120},
            {"exit_code": 0, "timeout": False, "spawn_ms": 4, "first_stdout_ms": 200, "total_ms": 240},
            {"exit_code": 1, "timeout": False, "spawn_ms": 1, "first_stdout_ms": 1, "total_ms": 1},
            {"exit_code": -9, "timeout": True, "spawn_ms": 1, "first_stdout_ms": None, "total_ms": 999},
        ]
        self.assertEqual(diagnostic.phase_stats(samples), {
            "successful_runs": 2, "attempts": 4, "spawn_ms": 3,
            "first_stdout_ms": 150, "total_ms": 180,
        })

    def test_phase_stats_missing_first_output(self):
        self.assertIsNone(diagnostic.phase_stats([
            {"exit_code": 0, "timeout": False, "spawn_ms": 1, "first_stdout_ms": None, "total_ms": 4},
        ])["first_stdout_ms"])

    def test_default_never_invokes_model(self):
        with mock.patch.object(diagnostic, "run_probe", return_value={"summary": {}}) as probe, \
             mock.patch.object(diagnostic.shutil, "which", return_value="/usr/bin/mock"), \
             mock.patch.object(diagnostic.sys, "platform", "linux"):
            result = diagnostic.diagnose(Path.cwd(), 2, 10, providers=["opencode"])
        self.assertFalse(result["model_requests_enabled"])
        self.assertEqual(probe.call_count, 3)
        self.assertNotIn("model_direct", result["providers"]["opencode"])

    def test_model_probe_only_when_explicitly_enabled(self):
        with mock.patch.object(diagnostic, "run_probe", return_value={"summary": {}}) as probe, \
             mock.patch.object(diagnostic.shutil, "which", return_value="/usr/bin/mock"), \
             mock.patch.object(diagnostic.sys, "platform", "linux"):
            result = diagnostic.diagnose(Path.cwd(), 2, 10, include_model=True, providers=["opencode"])
        self.assertTrue(result["model_requests_enabled"])
        self.assertEqual(probe.call_count, 4)
        self.assertIn("model_direct", result["providers"]["opencode"])

    def test_missing_provider_does_not_execute_version_or_model(self):
        with mock.patch.object(diagnostic, "run_probe", return_value={"summary": {}}) as probe, \
             mock.patch.object(diagnostic.shutil, "which", return_value=None):
            result = diagnostic.diagnose(Path.cwd(), 1, 10, include_model=True, providers=["copilot"])
        self.assertFalse(result["providers"]["copilot"]["available"])
        self.assertEqual(probe.call_count, 2)

    def test_sandbox_command_prepends_profile(self):
        with mock.patch.object(diagnostic, "measure", return_value={"exit_code": 0, "timeout": False}) as measure:
            diagnostic.run_probe(["echo", "OK"], Path.cwd(), 1, 10, "(version 1)")
        self.assertEqual(measure.call_args.args[0][:3], ["sandbox-exec", "-p", "(version 1)"])
