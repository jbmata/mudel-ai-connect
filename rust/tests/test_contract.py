"""Contrato Python/Rust y prueba del binario real por stdio, sin credenciales reales."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from mcp import Client, StdioServerParameters

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "shared"))
import moodle_mcp as python_server  # noqa: E402

BINARY = ROOT / "rust/target/debug" / ("mudel-ai-connect-server.exe" if sys.platform == "win32" else "mudel-ai-connect-server")
FIXTURE = json.loads((Path(__file__).parent / "fixtures/moodle.json").read_text(encoding="utf-8"))
CORE_TOOLS = set(FIXTURE["expected"])
TOOLS = CORE_TOOLS | {"read_course_file", "download_course_file"}


class ContractTests(unittest.TestCase):
    def test_python_still_matches_the_shared_contract(self):
        client = python_server.MoodleClient("https://campus.example/moodle", "secret123")
        with patch.object(client, "call", side_effect=lambda function, **params: FIXTURE["responses"][function]), \
                patch.object(python_server, "get_client", return_value=client), \
                patch.object(python_server, "require_permission"):
            actual = {name: getattr(python_server, name)(**({"course_id": 7} if name in {
                "get_course_context", "list_assignments"} else {})) for name in CORE_TOOLS}
        actual["get_course_context"].pop("fetched_at")
        self.assertEqual(actual, FIXTURE["expected"])

    def test_python_file_reads_match_the_shared_contract(self):
        cases = json.loads((Path(__file__).parent / "fixtures/file_reads.json").read_text(encoding="utf-8"))
        client = python_server.MoodleClient("https://campus.example/moodle", "secret123")
        for case in cases:
            with self.subTest(filename=case["filename"], offset=case["offset"]), \
                    patch.object(client, "call", return_value=case["sections"]), \
                    patch.object(client, "download", return_value=(case["body"].encode(), case["content_type"])), \
                    patch.object(python_server, "get_client", return_value=client), \
                    patch.object(python_server, "require_permission"):
                self.assertEqual(python_server.read_course_file(7, 90, offset=case["offset"]), case["expected"])

    def test_server_instructions_match_python(self):
        instructions = (ROOT / "rust/src/instructions.txt").read_text(encoding="utf-8")
        self.assertEqual(instructions, python_server.INSTRUCTIONS)

    def test_check_errors_are_utf8_and_do_not_echo_the_token(self):
        result = subprocess.run([str(BINARY), "--check"], env={**os.environ,
            "MOODLE_URL": "http://invalid.example", "MOODLE_TOKEN": "SecretNeverEcho987"},
            capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertIn("HTTPS", result.stderr.decode("utf-8"))
        self.assertNotIn(b"SecretNeverEcho987", result.stderr)


class StdioTests(unittest.IsolatedAsyncioTestCase):
    async def test_real_process_discovers_tools_and_rejects_invalid_calls(self):
        with tempfile.TemporaryDirectory() as temporary:
            params = StdioServerParameters(command=str(BINARY), env={
                "MOODLE_URL": "https://campus.example", "MOODLE_TOKEN": "",
                "LOCALAPPDATA": temporary, "XDG_DATA_HOME": temporary,
            })
            async with Client(params) as client:
                tools = (await client.list_tools()).tools
                self.assertEqual({tool.name for tool in tools}, TOOLS)
                for tool in tools:
                    self.assertEqual(tool.annotations.read_only_hint, tool.name != "download_course_file")
                    self.assertFalse(tool.annotations.destructive_hint)
                missing = await client.call_tool("moodle_status")
                self.assertTrue(missing.is_error)
                self.assertIn("MOODLE_TOKEN", missing.content[0].text)
                for name, arguments in [
                    ("get_course_context", {"course_id": -1}),
                    ("get_course_context", {"course_id": 7, "section_start": -1}),
                    ("get_course_context", {"course_id": 7, "section_limit": 11}),
                    ("list_assignments", {"course_id": 0}),
                    ("read_course_file", {"course_id": 7, "module_id": -1}),
                    ("download_course_file", {"course_id": 7, "module_id": 90, "path": "/tmp/arbitrary"}),
                ]:
                    with self.subTest(tool=name, arguments=arguments):
                        result = await client.call_tool(name, arguments)
                        self.assertTrue(result.is_error)
                self.assertIn("estudiar_curso", [prompt.name for prompt in (await client.list_prompts()).prompts])
                for arguments in ({"course_id": "7"}, {"course_id": "7", "pregunta": "Explica el tema 1"}):
                    result = await client.get_prompt("estudiar_curso", arguments)
                    expected = python_server.estudiar_curso(**arguments)
                    self.assertEqual(result.messages[0].content.text, expected)


if __name__ == "__main__":
    if not BINARY.is_file():
        raise SystemExit("Compila primero: cargo build --manifest-path rust/Cargo.toml --locked")
    unittest.main()
