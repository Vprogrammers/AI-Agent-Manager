import sys
from pathlib import Path
from unittest import TestCase, mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import trace_agent_phases as trace


class TraceAgentPhasesTests(TestCase):
    def test_phase_breakdown(self):
        samples = [{"spawn_ms": 4, "first_stdout_ms": 104, "total_ms": 154,
                    "exit_code": 0, "timeout": False}]
        s = trace.summarize(samples)
        self.assertEqual(s["phases_median_ms"], {
            "spawn_ms": 4, "cli_to_first_stdout_ms": 100,
            "first_stdout_to_exit_ms": 50, "total_ms": 154})
        self.assertEqual(s["successful"], 1)

    def test_no_stdout_does_not_invent_first_output(self):
        s = trace.summarize([{"spawn_ms": 2, "first_stdout_ms": None,
                              "total_ms": 50, "exit_code": 0, "timeout": False}])
        self.assertNotIn("cli_to_first_stdout_ms", s["phases_median_ms"])
        self.assertEqual(s["phases_median_ms"]["total_ms"], 50)

    def test_failures_and_timeouts_are_excluded(self):
        samples = [
            {"spawn_ms": 1, "first_stdout_ms": 2, "total_ms": 3,
             "exit_code": 1, "timeout": False},
            {"spawn_ms": 1, "first_stdout_ms": None, "total_ms": 100,
             "exit_code": -9, "timeout": True},
        ]
        s = trace.summarize(samples)
        self.assertEqual(s["successful"], 0)
        self.assertEqual(s["attempts"], 2)
        self.assertEqual(s["timeouts"], 1)
        self.assertEqual(s["phases_median_ms"], {})

    def test_model_is_opt_in_and_all_providers_are_selected(self):
        with mock.patch.object(trace.shutil, "which", return_value="/mock"), \
             mock.patch.object(trace.sys, "platform", "linux"), \
             mock.patch.object(trace, "timed_git", return_value={"elapsed_ms": 1, "exit_code": 0}), \
             mock.patch.object(trace, "measure", return_value={
                 "spawn_ms": 1, "first_stdout_ms": 2, "total_ms": 3,
                 "exit_code": 0, "timeout": False}) as measured:
            report = trace.run(Path.cwd(), 1, 5, False, list(trace.PROVIDERS))
        self.assertEqual(set(report["providers"]), set(trace.PROVIDERS))
        self.assertEqual(measured.call_count, len(trace.PROVIDERS))
        self.assertTrue(all(set(x["variants"]) == {"version_direct"}
                            for x in report["providers"].values()))

    def test_model_runs_only_when_requested(self):
        with mock.patch.object(trace.shutil, "which", return_value="/mock"), \
             mock.patch.object(trace.sys, "platform", "linux"), \
             mock.patch.object(trace, "timed_git", return_value={"elapsed_ms": 1, "exit_code": 0}), \
             mock.patch.object(trace, "measure", return_value={
                 "spawn_ms": 1, "first_stdout_ms": 2, "total_ms": 3,
                 "exit_code": 0, "timeout": False}) as measured:
            report = trace.run(Path.cwd(), 2, 5, True, ["codex"])
        self.assertEqual(measured.call_count, 4)
        self.assertEqual(set(report["providers"]["codex"]["variants"]),
                         {"version_direct", "model_direct"})
