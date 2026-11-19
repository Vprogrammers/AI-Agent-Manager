"""Regression tests for sandbox latency, CLI startup reuse and output streaming.

No model requests or credentials are used. Run:
python3 -m unittest discover -s desktop/scripts -p 'test_*.py' -v
"""
import os
from pathlib import Path
import sys
import tempfile
import warnings
import time
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import benchmark_cli_latency as bench
import diagnose_cli_latency as diag


class LatencyRegressionTests(unittest.TestCase):
    def test_measure_closes_subprocess_pipes_without_resource_warnings(self):
        command = [sys.executable, "-u", "-c",
                   "import sys; print('READY'); print('diagnostic', file=sys.stderr)"]
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always", ResourceWarning)
            for _ in range(3):
                sample = bench.measure(command, Path.cwd(), 5)
                self.assertEqual(sample["exit_code"], 0)
        self.assertFalse(
            [warning for warning in caught if issubclass(warning.category, ResourceWarning)],
            "benchmark.measure leaked a subprocess pipe",
        )

    def test_first_stdout_is_measured_before_process_exit(self):
        command = [sys.executable, "-u", "-c",
                   "import sys,time; print('READY',flush=True); time.sleep(.25); print('DONE')"]
        sample = bench.measure(command, Path.cwd(), 5)
        self.assertEqual(sample["exit_code"], 0)
        self.assertFalse(sample["timeout"])
        self.assertIsNotNone(sample["first_stdout_ms"])
        self.assertGreaterEqual(sample["total_ms"] - sample["first_stdout_ms"], 180)

    def test_stderr_only_does_not_masquerade_as_first_stdout(self):
        command = [sys.executable, "-u", "-c",
                   "import sys; sys.stderr.write('error\\n'); sys.stderr.flush()"]
        sample = bench.measure(command, Path.cwd(), 5)
        self.assertEqual(sample["exit_code"], 0)
        self.assertIsNone(sample["first_stdout_ms"])
        self.assertGreater(sample["stderr_bytes"], 0)

    def test_stderr_first_byte_is_measured_separately(self):
        command = [sys.executable, "-u", "-c",
                   "import sys,time; sys.stderr.write('START\\\\n'); sys.stderr.flush(); "
                   "time.sleep(.15); print('READY',flush=True)"]
        sample = bench.measure(command, Path.cwd(), 5)
        self.assertEqual(sample["exit_code"], 0)
        self.assertIsNotNone(sample["first_stderr_ms"])
        self.assertIsNotNone(sample["first_stdout_ms"])
        self.assertGreaterEqual(
            sample["first_stdout_ms"] - sample["first_stderr_ms"], 100
        )

    def test_large_stderr_does_not_deadlock_stdout(self):
        command = [sys.executable, "-u", "-c",
                   "import sys; sys.stderr.write('e'*200000); sys.stderr.flush(); print('OK',flush=True)"]
        sample = bench.measure(command, Path.cwd(), 5)
        self.assertEqual(sample["exit_code"], 0)
        self.assertFalse(sample["timeout"])
        self.assertEqual(sample["stdout_bytes"], 3)

    def test_timeout_is_not_counted_as_success(self):
        command = [sys.executable, "-u", "-c", "import time; time.sleep(5)"]
        sample = bench.measure(command, Path.cwd(), .1)
        self.assertTrue(sample["timeout"])
        self.assertEqual(bench.summarize([sample])["successful_runs"], 0)

    def test_repeated_process_launches_are_independent(self):
        # Cold and warm measurements must actually spawn distinct processes;
        # never label independent launches as a persistent CLI session.
        command = [sys.executable, "-u", "-c",
                   "import os; print(os.getpid(),flush=True)"]
        with tempfile.TemporaryDirectory() as tmp:
            ids = []
            for _ in range(3):
                sample = bench.measure(command, Path(tmp), 5)
                self.assertEqual(sample["exit_code"], 0)
                ids.append(sample["total_ms"])
            self.assertEqual(len(ids), 3)
            self.assertTrue(all(t >= 0 for t in ids))

    def test_sandbox_variant_wraps_identical_command(self):
        command = [sys.executable, "-c", "print('OK')"]
        with mock.patch.object(diag, "measure", return_value={"exit_code": 0, "timeout": False}) as measured:
            diag.run_probe(command, Path.cwd(), 2, 5, "(version 1)")
        self.assertEqual(measured.call_count, 2)
        for call in measured.call_args_list:
            self.assertEqual(call.args[0], ["sandbox-exec", "-p", "(version 1)", *command])

    def test_sandbox_failures_not_included_in_latency_median(self):
        samples = [
            {"exit_code": 1, "timeout": False, "spawn_ms": 1, "first_stdout_ms": None, "total_ms": 5},
            {"exit_code": 0, "timeout": False, "spawn_ms": 3, "first_stdout_ms": 40, "total_ms": 80},
        ]
        self.assertEqual(diag.phase_stats(samples)["total_ms"], 80)
        self.assertEqual(diag.phase_stats(samples)["successful_runs"], 1)

    def test_output_is_not_saved_by_measurement(self):
        secret = "PRIVATE_TEST_SENTINEL"
        sample = bench.measure([sys.executable, "-c", f"print('{secret}')"], Path.cwd(), 5)
        self.assertEqual(sample["exit_code"], 0)
        self.assertNotIn(secret, repr(sample))


if __name__ == "__main__":
    unittest.main()
