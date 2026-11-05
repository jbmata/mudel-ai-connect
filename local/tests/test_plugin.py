import json
import os
import subprocess
import sys
import tempfile
import unittest
import venv
from pathlib import Path
from unittest.mock import Mock, patch
from zipfile import ZipFile

from mcp import Client, StdioServerParameters
from mcp.server.mcpserver.exceptions import ToolError

sys.path[:0] = [str(Path(__file__).resolve().parents[1]), str(Path(__file__).resolve().parents[2] / "shared")]

import moodle_mcp  # noqa: E402
import plugin_runtime as runtime  # noqa: E402
from scripts.build_plugin import FILES, README, ROOT, SOURCES, build  # noqa: E402


class PluginTests(unittest.TestCase):
    def test_manifests_include_local_mcp_and_matching_identity(self):
        portable = json.loads((ROOT / SOURCES["plugin.json"]).read_text(encoding="utf-8"))
        claude = json.loads((ROOT / SOURCES[".claude-plugin/plugin.json"]).read_text(encoding="utf-8"))
        self.assertEqual((portable["name"], portable["version"]), (claude["name"], claude["version"]))
        for name, variable in [("mcp.json", "PLUGIN_ROOT"), (".mcp.json", "CLAUDE_PLUGIN_ROOT")]:
            config = json.loads((ROOT / SOURCES[name]).read_text(encoding="utf-8"))["mcpServers"]["mudel-ai-connect"]
            self.assertEqual(config["type"], "stdio")
            args = [arg.replace("${" + variable + "}", str(ROOT / "local")) for arg in config["args"]]
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

    def test_malformed_encrypted_credentials_are_reported_as_value_errors(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(runtime, "data_dir", return_value=Path(temporary)):
            for value in ([], None, {"MOODLE_URL": "https://campus.example", "MOODLE_TOKEN_DPAPI": None},
                          {"MOODLE_URL": 42, "MOODLE_TOKEN_DPAPI": "PRIVATE_CANARY"}):
                runtime.protected_credentials_path().write_text(json.dumps(value), encoding="utf-8")
                with self.subTest(value=value), self.assertRaises(ValueError) as caught:
                    runtime.read_credentials(protect=True)
                self.assertNotIn("PRIVATE_CANARY", str(caught.exception))

    def test_plugin_and_installer_use_their_own_credentials(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(runtime, "data_dir", return_value=Path(temporary)), \
                patch.object(runtime, "_dpapi", side_effect=lambda data, protect: data):
            runtime.save_credentials("https://installer.example", "InstallerToken123", protect=True)
            runtime.save_credentials("https://plugin.example", "PluginToken123")
            self.assertEqual(runtime.read_credentials(protect=True)["MOODLE_TOKEN"], "InstallerToken123")
            self.assertEqual(runtime.read_credentials(protect=False)["MOODLE_TOKEN"], "PluginToken123")
            with patch.object(sys, "frozen", True, create=True):
                self.assertEqual(moodle_mcp.read_credentials()["MOODLE_TOKEN"], "InstallerToken123")
                runtime.protected_credentials_path().unlink()
                self.assertEqual(moodle_mcp.read_credentials(), {})

    def test_plugin_launcher_passes_only_its_own_saved_account(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(runtime, "data_dir", return_value=Path(temporary)), \
                patch.object(runtime, "_dpapi", side_effect=lambda data, protect: data):
            folder = Path(temporary)
            python = folder / "runtime/Scripts/python.exe"
            python.parent.mkdir(parents=True)
            python.touch()
            (python.parent.parent / ".mudel-ai-connect-ready").touch()
            runtime.save_credentials("https://installer.example", "InstallerToken123", protect=True)
            runtime.save_credentials("https://plugin.example", "PluginToken123")
            with patch.object(runtime, "runtime_python", return_value=python), patch.dict(os.environ, {}, clear=True), \
                    patch.object(runtime.subprocess, "call", return_value=0) as launch:
                self.assertEqual(runtime.run(), 0)
                environment = launch.call_args.kwargs["env"]
                self.assertEqual(environment["MOODLE_URL"], "https://plugin.example")
                self.assertEqual(environment["MOODLE_TOKEN"], "PluginToken123")
                self.assertNotIn("PluginToken123", " ".join(launch.call_args.args[0]))
                runtime.credentials_path().unlink()
                runtime.run()
                self.assertEqual(launch.call_args.kwargs["env"]["MOODLE_TOKEN"], "")

    def test_process_environment_takes_priority_over_saved_configuration(self):
        with patch.dict(os.environ, {"MOODLE_URL": "https://campus.example", "MOODLE_TOKEN": "EnvironmentToken123"}), \
                patch.object(moodle_mcp, "read_credentials") as saved:
            self.assertEqual(moodle_mcp.get_client().token, "EnvironmentToken123")
            saved.assert_not_called()

    def test_saved_configuration_is_used_by_installed_copy(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(moodle_mcp, "read_credentials", return_value={
            "MOODLE_URL": "https://campus.example", "MOODLE_TOKEN": "SavedToken123"}):
            client = moodle_mcp.get_client()
            self.assertEqual(client.base_url, "https://campus.example")
            self.assertEqual(client.token, "SavedToken123")

    def test_saved_configuration_errors_are_reported(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(moodle_mcp, "read_credentials",
                side_effect=ValueError("No se pudo leer la configuración local.")):
            with self.assertRaisesRegex(ToolError, "No se pudo leer la configuración local"):
                moodle_mcp.get_client()

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
            (directory / "plugin_runtime.py").write_bytes((ROOT / SOURCES["plugin_runtime.py"]).read_bytes())
            (directory / "requirements.txt").write_text("", encoding="utf-8")
            (directory / "moodle_mcp.py").write_text(
                "import sys,time; sys.stdin.read(); time.sleep(0.2); print('cerrado'); sys.exit(7)",
                encoding="utf-8",
            )
            env = dict(os.environ, LOCALAPPDATA=str(directory / "state"), XDG_DATA_HOME=str(directory / "state"))
            with patch.object(runtime, "ROOT", directory), patch.dict(os.environ, env):
                python = runtime.runtime_python()
            venv.EnvBuilder(with_pip=False).create(python.parent.parent)
            (python.parent.parent / ".mudel-ai-connect-ready").touch()
            result = subprocess.run([sys.executable, str(directory / "plugin_runtime.py"), "run"],
                                    input="", capture_output=True, text=True, env=env, timeout=20)
            self.assertEqual(result.returncode, 7)
            self.assertEqual(result.stdout.strip(), "cerrado")

    def test_package_and_marketplace_exclude_private_and_extra_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for source in (*SOURCES.values(), README):
                target = root / source
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((ROOT / source).read_bytes())
            (root / ".env").write_text("MOODLE_TOKEN=PRIVATE_CANARY_579", encoding="utf-8")
            (root / "private-notes.txt").write_text("PRIVATE_CANARY_579", encoding="utf-8")
            archive_path = build(root)
            with ZipFile(archive_path) as archive:
                self.assertEqual(set(archive.namelist()), set(FILES) | {"README.md"})
                self.assertEqual(archive.read("LICENSE"), (ROOT / "LICENSE").read_bytes())
                for name in archive.namelist():
                    self.assertNotIn(b"PRIVATE_CANARY_579", archive.read(name))
            marketplace = root / "dist/mudel-ai-connect-marketplace"
            self.assertFalse((marketplace / "plugins/mudel-ai-connect/.env").exists())
            catalog = json.loads((marketplace / ".agents/plugins/marketplace.json").read_text(encoding="utf-8"))
            self.assertEqual(catalog["plugins"][0]["name"], "mudel-ai-connect")
            extra = marketplace / "plugins/mudel-ai-connect/.env"
            extra.write_text("PRIVATE_CANARY_579", encoding="utf-8")
            with self.assertRaises(ValueError):
                build(root)


class ExtractedPluginTests(unittest.IsolatedAsyncioTestCase):
    async def test_extracted_server_discovers_tools_without_local_secrets(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            # Usar una copia aislada: el servidor extraído no tiene el .env del proyecto.
            for name in ("moodle_mcp.py", "plugin_runtime.py"):
                (directory / name).write_bytes((ROOT / SOURCES[name]).read_bytes())
            params = StdioServerParameters(
                command=os.sys.executable, args=[str(directory / "moodle_mcp.py")], cwd=directory,
                env={"MOODLE_URL": "https://campus.example", "MOODLE_TOKEN": "", "PYTHONIOENCODING": "utf-8"},
            )
            async with Client(params) as client:
                self.assertEqual(len((await client.list_tools()).tools), 6)
                status = await client.call_tool("moodle_status")
                self.assertTrue(status.is_error)
                self.assertIn("MOODLE_TOKEN", status.content[0].text)


class PermissionTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.folder = Path(self.directory.name)
        patcher = patch.object(runtime, "data_dir", return_value=self.folder)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_defaults_and_saved_permissions_preserve_credentials(self):
        self.assertEqual(runtime.read_permissions(), dict.fromkeys(runtime.PERMISSIONS, True))
        runtime.save_credentials("https://campus.example", "SavedToken123")
        allowed = dict.fromkeys(runtime.PERMISSIONS, False)
        allowed["courses"] = True
        runtime.save_permissions(allowed)
        self.assertEqual(runtime.read_permissions(), allowed)
        self.assertEqual(runtime.read_credentials()["MOODLE_TOKEN"], "SavedToken123")

    def test_disabled_queries_stop_before_creating_a_moodle_client(self):
        queries = (
            ("courses", moodle_mcp.list_courses, ()),
            ("materials", moodle_mcp.get_course_context, (7,)),
            ("materials", moodle_mcp.read_course_file, (7, 90)),
            ("assignments", moodle_mcp.list_assignments, (7,)),
            ("calendar", moodle_mcp.get_upcoming_events, ()),
        )
        for permission, query, arguments in queries:
            allowed = dict.fromkeys(runtime.PERMISSIONS, True)
            allowed[permission] = False
            runtime.save_permissions(allowed)
            with self.subTest(permission=permission, query=query.__name__), \
                    patch.object(moodle_mcp, "get_client") as client:
                with self.assertRaisesRegex(ToolError, "Consulta desactivada"):
                    query(*arguments)
                client.assert_not_called()
                for other in runtime.PERMISSIONS.keys() - {permission}:
                    moodle_mcp.require_permission(other)

    def test_invalid_permissions_are_rejected_without_replacing_the_previous_selection(self):
        allowed = dict.fromkeys(runtime.PERMISSIONS, False)
        runtime.save_permissions(allowed)
        for invalid in (None, [], {}, {**allowed, "calendar": "false"},
                        {**allowed, "courses": 1}, {**allowed, "write": True}):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                runtime.save_permissions(invalid)
            self.assertEqual(runtime.read_permissions(), allowed)
        (self.folder / "permissions.json").write_text("PRIVATE_CANARY_invalid_json", encoding="utf-8")
        with patch.object(moodle_mcp, "get_client") as client:
            with self.assertRaisesRegex(ToolError, "No se pudieron leer los permisos") as caught:
                moodle_mcp.list_courses()
            client.assert_not_called()
            self.assertNotIn("PRIVATE_CANARY", str(caught.exception))
        (self.folder / "permissions.json").unlink()
        with patch.object(Path, "read_text", side_effect=PermissionError):
            with self.assertRaisesRegex(ValueError, "No se pudieron leer los permisos"):
                runtime.read_permissions()

    def test_failed_save_preserves_the_previous_selection(self):
        allowed = dict.fromkeys(runtime.PERMISSIONS, False)
        runtime.save_permissions(allowed)
        with patch.object(runtime.os, "replace", side_effect=OSError), self.assertRaises(OSError):
            runtime.save_permissions(dict.fromkeys(runtime.PERMISSIONS, True))
        self.assertEqual(runtime.read_permissions(), allowed)
        self.assertEqual(list(self.folder.iterdir()), [self.folder / "permissions.json"])

    async def test_revocation_applies_to_an_already_open_mcp_connection(self):
        allowed = dict.fromkeys(runtime.PERMISSIONS, True)
        runtime.save_permissions(allowed)
        moodle = Mock()
        moodle.call.side_effect = [{"userid": 3}, []]
        moodle.clean.side_effect = lambda result: result
        with patch.object(moodle_mcp, "get_client", return_value=moodle) as client:
            async with Client(moodle_mcp.mcp) as connection:
                result = await connection.call_tool("list_courses")
                self.assertFalse(result.is_error)
                allowed["courses"] = False
                runtime.save_permissions(allowed)
                blocked = await connection.call_tool("list_courses")
                self.assertTrue(blocked.is_error)
                self.assertIn("Consulta desactivada", blocked.content[0].text)
                client.assert_called_once()


if __name__ == "__main__":
    unittest.main()
