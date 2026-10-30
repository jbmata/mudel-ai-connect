import json
import os
import subprocess
import sys
import tempfile
import unittest
import venv
from pathlib import Path
from unittest.mock import patch
from zipfile import ZipFile

from mcp import Client, StdioServerParameters

import moodle_mcp
import plugin_runtime as runtime
from scripts.build_plugin import FILES, ROOT, build


class PluginTests(unittest.TestCase):
    def test_manifests_include_local_mcp_and_matching_identity(self):
        portable = json.loads((ROOT / "plugin.json").read_text(encoding="utf-8"))
        claude = json.loads((ROOT / ".claude-plugin/plugin.json").read_text(encoding="utf-8"))
        self.assertEqual((portable["name"], portable["version"]), (claude["name"], claude["version"]))
        for name, variable in [("mcp.json", "PLUGIN_ROOT"), (".mcp.json", "CLAUDE_PLUGIN_ROOT")]:
            config = json.loads((ROOT / name).read_text(encoding="utf-8"))["mcpServers"]["moodle"]
            self.assertEqual(config["type"], "stdio")
            args = [arg.replace("${" + variable + "}", str(ROOT)) for arg in config["args"]]
            self.assertTrue(Path(args[0]).is_file())
            self.assertNotIn("env", config)

    def test_credentials_roundtrip_outside_plugin_and_validation(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(runtime, "data_dir", return_value=Path(temporary)):
            self.assertEqual(runtime.read_credentials(), {})
            runtime.save_credentials("https://campus.example/", "LocalToken123")
            self.assertEqual(runtime.read_credentials(), {
                "MOODLE_URL": "https://campus.example", "MOODLE_TOKEN": "LocalToken123"})
            for url, token in [("http://campus.example", "Token"), ("https://campus.example", ""),
                               ("https://user:password@campus.example", "Token")]:
                with self.subTest(url=url), self.assertRaises(ValueError):
                    runtime.save_credentials(url, token)
            self.assertEqual(list(Path(temporary).iterdir()), [runtime.credentials_path()])

    def test_invalid_config_does_not_echo_its_content(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(runtime, "data_dir", return_value=Path(temporary)):
            runtime.credentials_path().write_text("PRIVATE_SECRET_not_json", encoding="utf-8")
            with self.assertRaises(ValueError) as caught:
                runtime.read_credentials()
            self.assertNotIn("PRIVATE_SECRET", str(caught.exception))

    def test_process_environment_takes_priority_over_saved_configuration(self):
        with patch.dict(os.environ, {"MOODLE_URL": "https://campus.example", "MOODLE_TOKEN": "EnvironmentToken123"}), \
                patch.object(moodle_mcp, "read_credentials") as saved:
            self.assertEqual(moodle_mcp.get_client().token, "EnvironmentToken123")
            saved.assert_not_called()

    def test_saved_configuration_is_used_by_installed_copy(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(moodle_mcp, "read_credentials", return_value={
            "MOODLE_URL": "https://campus.example", "MOODLE_TOKEN": "SavedToken123"}):
            self.assertEqual(moodle_mcp.get_client().token, "SavedToken123")

    def test_partial_installation_is_not_launched(self):
        with tempfile.TemporaryDirectory() as temporary:
            python = Path(temporary) / "Scripts" / "python.exe"
            python.parent.mkdir()
            python.touch()
            with patch.object(runtime, "runtime_python", return_value=python), self.assertRaises(ValueError):
                runtime.run()

    def test_launcher_waits_for_child_and_propagates_its_exit_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "plugin_runtime.py").write_bytes((ROOT / "plugin_runtime.py").read_bytes())
            (directory / "requirements.txt").write_text("", encoding="utf-8")
            (directory / "moodle_mcp.py").write_text(
                "import sys,time; sys.stdin.read(); time.sleep(0.2); print('cerrado'); sys.exit(7)",
                encoding="utf-8",
            )
            env = dict(os.environ, LOCALAPPDATA=str(directory / "state"), XDG_DATA_HOME=str(directory / "state"))
            with patch.object(runtime, "ROOT", directory), patch.dict(os.environ, env):
                python = runtime.runtime_python()
            venv.EnvBuilder(with_pip=False).create(python.parent.parent)
            (python.parent.parent / ".moodle-ready").touch()
            result = subprocess.run([sys.executable, str(directory / "plugin_runtime.py"), "run"],
                                    input="", capture_output=True, text=True, env=env, timeout=20)
            self.assertEqual(result.returncode, 7)
            self.assertEqual(result.stdout.strip(), "cerrado")

    def test_package_and_marketplace_exclude_private_and_extra_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in (*FILES, "PLUGIN.md"):
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((ROOT / name).read_bytes())
            (root / ".env").write_text("MOODLE_TOKEN=PRIVATE_CANARY_579", encoding="utf-8")
            (root / "private-notes.txt").write_text("PRIVATE_CANARY_579", encoding="utf-8")
            archive_path = build(root)
            with ZipFile(archive_path) as archive:
                self.assertEqual(set(archive.namelist()), set(FILES) | {"README.md"})
                for name in archive.namelist():
                    self.assertNotIn(b"PRIVATE_CANARY_579", archive.read(name))
            marketplace = root / "dist/moodle-marketplace"
            self.assertFalse((marketplace / "plugins/moodle/.env").exists())
            catalog = json.loads((marketplace / ".agents/plugins/marketplace.json").read_text(encoding="utf-8"))
            self.assertEqual(catalog["plugins"][0]["name"], "moodle")
            extra = marketplace / "plugins/moodle/.env"
            extra.write_text("PRIVATE_CANARY_579", encoding="utf-8")
            with self.assertRaises(ValueError):
                build(root)


class ExtractedPluginTests(unittest.IsolatedAsyncioTestCase):
    async def test_extracted_server_discovers_tools_without_local_secrets(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            # Usar una copia aislada: el servidor extraído no tiene el .env del proyecto.
            for name in ("moodle_mcp.py", "plugin_runtime.py"):
                (directory / name).write_bytes((ROOT / name).read_bytes())
            params = StdioServerParameters(
                command=os.sys.executable, args=[str(directory / "moodle_mcp.py")], cwd=directory,
                env={"MOODLE_URL": "https://campus.example", "MOODLE_TOKEN": "", "PYTHONIOENCODING": "utf-8"},
            )
            async with Client(params) as client:
                self.assertEqual(len((await client.list_tools()).tools), 6)
                status = await client.call_tool("moodle_status")
                self.assertTrue(status.is_error)
                self.assertIn("MOODLE_TOKEN", status.content[0].text)


if __name__ == "__main__":
    unittest.main()
