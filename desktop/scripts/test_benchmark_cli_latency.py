import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).with_name("benchmark_cli_latency.py")
spec = importlib.util.spec_from_file_location("benchmark_cli_latency", SCRIPT)
benchmark = importlib.util.module_from_spec(spec)
spec.loader.exec_module(benchmark)


class BenchmarkTests(unittest.TestCase):
    def test_provider_commands_match_aam_external_adapter(self):
        prompt = "Odgovori samo OK"
        self.assertEqual(benchmark.PROVIDERS["opencode"](prompt), ["opencode", "run", prompt])
        self.assertEqual(benchmark.PROVIDERS["copilot"](prompt),
                         ["copilot", "-p", prompt, "-s", "--no-ask-user"])
        self.assertEqual(benchmark.PROVIDERS["antigravity"](prompt), ["agy", "--print", prompt])

    def test_summary_excludes_failed_and_timeout_samples(self):
        samples = [
            {"total_ms": 100, "exit_code": 0, "timeout": False},
            {"total_ms": 300, "exit_code": 0, "timeout": False},
            {"total_ms": 1, "exit_code": 1, "timeout": False},
            {"total_ms": 10, "exit_code": -9, "timeout": True},
        ]
        self.assertEqual(benchmark.summarize(samples),
                         {"successful_runs": 2, "median_ms": 200, "min_ms": 100, "max_ms": 300})

    def test_summary_handles_all_failed(self):
        self.assertEqual(benchmark.summarize([{"exit_code": 1, "total_ms": 9}]),
                         {"successful_runs": 0})

    def test_measure_records_first_stdout_and_exit(self):
        import sys
        result = benchmark.measure(
            [sys.executable, "-c", "print('OK', flush=True)"],
            Path.cwd(), 10)
        self.assertEqual(result["exit_code"], 0)
        self.assertFalse(result["timeout"])
        self.assertIsNotNone(result["first_stdout_ms"])
        self.assertGreater(result["stdout_bytes"], 0)
        self.assertGreaterEqual(result["total_ms"], result["spawn_ms"])


if __name__ == "__main__":
    unittest.main()
