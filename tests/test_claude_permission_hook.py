import io
import json
import os
import socket
import sys
import tempfile
import threading
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from ai_agent_manager import build_command, permission_hook


@unittest.skipUnless(hasattr(socket, "AF_UNIX"), "Unix socket permission bridge")
class ClaudePermissionHookTests(unittest.TestCase):
    def invoke_hook(self, allow, serve=True):
        with tempfile.TemporaryDirectory() as folder:
            address = os.path.join(folder, "approval.sock")
            received = []
            server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            if serve:
                server.bind(address)
                server.listen(1)

                def handle():
                    client, _ = server.accept()
                    with client:
                        payload = client.recv(65536)
                        received.append(json.loads(payload))
                        client.sendall(json.dumps({"allow": allow}).encode() + b"\n")

                worker = threading.Thread(target=handle, daemon=True)
                worker.start()
            data = {"tool_name": "Bash", "tool_input": {"command": "npm test"}}
            fake_input = io.TextIOWrapper(io.BytesIO(json.dumps(data).encode()))
            output = io.StringIO()
            with patch.dict(os.environ, {
                "AAM_APPROVAL_SOCKET": address, "AAM_APPROVAL_TOKEN": "test-token"
            }), patch("sys.stdin", fake_input), redirect_stdout(output):
                self.assertEqual(permission_hook(), 0)
            if serve:
                worker.join(timeout=2)
            server.close()
            return json.loads(output.getvalue()), received

    def test_allow_returns_original_tool_input(self):
        result, requests = self.invoke_hook(True)
        self.assertEqual(result["hookSpecificOutput"]["permissionDecision"], "allow")
        self.assertEqual(result["hookSpecificOutput"]["updatedInput"], {"command": "npm test"})
        self.assertEqual(requests[0]["token"], "test-token")
        self.assertEqual(requests[0]["tool"], "Bash")

    def test_deny_returns_native_deny(self):
        result, _ = self.invoke_hook(False)
        self.assertEqual(result["hookSpecificOutput"]["permissionDecision"], "deny")
        self.assertNotIn("updatedInput", result["hookSpecificOutput"])

    def test_unavailable_host_fails_closed(self):
        result, _ = self.invoke_hook(False, serve=False)
        self.assertEqual(result["hookSpecificOutput"]["permissionDecision"], "deny")

    def test_session_local_settings_are_not_persisted(self):
        with patch.dict(os.environ, {
            "AAM_PERMISSION_MODE": "workspace-write",
            "AAM_APPROVAL_SOCKET": "/tmp/aam-test.sock",
            "AAM_APPROVAL_TOKEN": "token",
        }):
            command = build_command("claude", None, None, "Fix tests")
            settings = json.loads(command[command.index("--settings") + 1])
            self.assertIn("PermissionRequest", settings["hooks"])
            self.assertIn("--approval-hook", settings["hooks"]["PermissionRequest"][0]["hooks"][0]["command"])


if __name__ == "__main__":
    unittest.main()
