import json
import os
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(REPO / "local"), str(REPO / "shared")]

import moodle_ai_connect_installer as installer  # noqa: E402
import plugin_runtime as runtime  # noqa: E402

COMMAND = [r"C:\Users\ana\AppData\Local\moodle-ai-connect\bin\moodle-ai-connect-server\moodle-ai-connect-server.exe"]


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        environment = {"USERPROFILE": str(self.home), "APPDATA": str(self.home / "Roaming"),
                       "LOCALAPPDATA": str(self.home / "Local")}
        patcher = patch.dict(os.environ, environment)
        patcher.start()
        self.addCleanup(patcher.stop)

    def client(self, key, name, path, kind):
        return installer.Client(key, name, path, kind)

    def test_detects_only_installed_clients_including_store_claude(self):
        self.assertEqual(installer.detect_clients(), [])
        store = self.home / "Local/Packages/Claude_abc/LocalCache/Roaming/Claude"
        store.mkdir(parents=True)
        (self.home / ".claude.json").write_text("{}", encoding="utf-8")
        (self.home / ".codex").mkdir()
        found = installer.detect_clients()
        self.assertEqual([item.name for item in found],
                         ["Claude Desktop", "Claude Code", "ChatGPT Work local / Codex"])
        self.assertEqual(found[0].path, store / "claude_desktop_config.json")

    def test_json_registration_keeps_other_servers_and_settings(self):
        path = self.home / "claude_desktop_config.json"
        path.write_text(json.dumps({"theme": "dark", "mcpServers": {"otro": {"command": "x"}}}),
                        encoding="utf-8")
        client = self.client("claude-desktop-0", "Claude Desktop", path, "json")
        installer.register(client, COMMAND)
        config = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(config["theme"], "dark")
        self.assertEqual(config["mcpServers"]["otro"], {"command": "x"})
        self.assertEqual(config["mcpServers"]["moodle-ai-connect"], {"command": COMMAND[0], "args": [], "env": {}})
        self.assertTrue(path.with_name(path.name + ".moodle-ai-connect-backup").is_file())
        self.assertTrue(installer.unregister(client))
        self.assertEqual(json.loads(path.read_text(encoding="utf-8"))["mcpServers"], {"otro": {"command": "x"}})
        self.assertFalse(installer.unregister(client))

    def test_claude_code_entry_declares_stdio_and_missing_file_is_created(self):
        path = self.home / "nuevo" / "config.json"
        installer.register(self.client("claude-code", "Claude Code", path, "json"), COMMAND)
        entry = json.loads(path.read_text(encoding="utf-8"))["mcpServers"]["moodle-ai-connect"]
        self.assertEqual(entry["type"], "stdio")

    def test_invalid_json_is_never_overwritten(self):
        path = self.home / "claude_desktop_config.json"
        path.write_text("{no es json", encoding="utf-8")
        with self.assertRaises(installer.InstallError):
            installer.register(self.client("claude-desktop-0", "Claude Desktop", path, "json"), COMMAND)
        self.assertEqual(path.read_text(encoding="utf-8"), "{no es json")

    def test_toml_registration_replaces_only_the_moodle_section(self):
        path = self.home / ".codex" / "config.toml"
        path.parent.mkdir()
        path.write_text('model = "x"\n\n[mcp_servers.moodle-ai-connect]\ncommand = "viejo"\n\n'
                        '[mcp_servers.moodle-ai-connect.env]\nA = "1"\n\n[mcp_servers.moodle-ai-connect-otro]\ncommand = "keep"\n\n'
                        '[projects.p]\ntrust = true\n', encoding="utf-8")
        client = self.client("codex", "Codex", path, "toml")
        installer.register(client, COMMAND)
        installer.register(client, COMMAND)  # idempotente
        config = tomllib.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(config["model"], "x")
        self.assertEqual(config["mcp_servers"]["moodle-ai-connect"], {"command": COMMAND[0], "args": []})
        self.assertEqual(config["mcp_servers"]["moodle-ai-connect-otro"], {"command": "keep"})
        self.assertEqual(config["projects"]["p"], {"trust": True})
        self.assertEqual(path.read_text(encoding="utf-8").count("[mcp_servers.moodle-ai-connect]"), 1)
        self.assertTrue(installer.unregister(client))
        self.assertNotIn("moodle-ai-connect", tomllib.loads(path.read_text(encoding="utf-8"))["mcp_servers"])

    def test_claude_desktop_is_found_by_install_path_never_claude_code(self):
        processes = {100: r"C:\Program Files\WindowsApps\Claude_4.1_x64__abc\app\claude.exe",
                     101: r"C:\Users\ana\.vscode\extensions\anthropic.claude-code-2\native-binary\claude.exe",
                     102: r"C:\Users\ana\.local\bin\claude.exe",
                     104: r"C:\Users\ana\AppData\Local\AnthropicClaude\app-1\claude.exe"}
        with patch.object(installer, "_process_paths", return_value=processes):
            self.assertEqual(installer.running_claude_desktop(), [100, 104])

    def test_install_refuses_while_claude_desktop_runs_and_writes_nothing(self):
        path = self.home / "claude_desktop_config.json"
        path.write_text("{}", encoding="utf-8")
        client = self.client("claude-desktop-0", "Claude Desktop", path, "json")
        with patch.object(installer, "running_claude_desktop", return_value=[100]), \
                patch.object(installer, "install_server") as copy, self.assertRaises(installer.InstallError):
            installer.install("https://sifo.comillas.edu", "abc123", [client])
        copy.assert_not_called()
        self.assertEqual(path.read_text(encoding="utf-8"), "{}")

    def test_install_checks_each_client_really_has_the_entry(self):
        path = self.home / "claude_desktop_config.json"
        client = self.client("claude-desktop-0", "Claude Desktop", path, "json")
        with patch.object(installer, "running_claude_desktop", return_value=[]), \
                patch.object(installer, "install_server", return_value=COMMAND), \
                patch.object(installer, "save_credentials"), patch.object(installer, "register"), \
                self.assertRaises(installer.InstallError):  # register simulado: no escribió nada
            installer.install("https://sifo.comillas.edu", "abc123", [client])

    def test_close_waits_until_no_claude_desktop_process_is_left(self):
        with patch.object(installer, "running_claude_desktop", side_effect=[[100, 101], [100], []]), \
                patch("subprocess.run") as kill, patch("time.sleep"):
            self.assertTrue(installer.close_claude_desktop())
        self.assertEqual([call.args[0][-1] for call in kill.call_args_list], ["100", "101"])

    def test_codex_is_not_preselected_when_it_already_has_the_moodle_plugin(self):
        (self.home / ".codex").mkdir()
        (self.home / ".codex" / "config.toml").write_text(
            '[plugins."moodle-ai-connect@moodle-ai-connect-local"]\nenabled = true\n', encoding="utf-8")
        codex = installer.detect_clients()[0]
        self.assertFalse(codex.selected)
        self.assertIn("ya tiene", codex.hint)
        (self.home / ".codex" / "config.toml").write_text('model = "x"\n', encoding="utf-8")
        self.assertTrue(installer.detect_clients()[0].selected)
        # También el plugin instalado con el nombre anterior.
        (self.home / ".codex" / "config.toml").write_text(
            '[plugins."moodle@moodle-local"]\nenabled = true\n', encoding="utf-8")
        self.assertFalse(installer.detect_clients()[0].selected)

    def test_client_state_tells_installed_from_outdated_from_missing(self):
        server = self.home / "bin" / "server.exe"
        server.parent.mkdir()
        server.write_text("exe", encoding="utf-8")
        path = self.home / "claude_desktop_config.json"
        client = self.client("claude-desktop-0", "Claude Desktop", path, "json")
        with patch.object(installer, "server_command", return_value=[str(server)]):
            self.assertEqual(installer.client_state(client), "missing")  # sin archivo
            path.write_text('{"mcpServers": {"otro": {"command": "x"}}}', encoding="utf-8")
            self.assertEqual(installer.client_state(client), "missing")  # sin nuestra entrada
            installer.register(client, [str(server)])
            self.assertEqual(installer.client_state(client), "ok")
            installer.register(client, [str(self.home / "antiguo.exe")])
            self.assertEqual(installer.client_state(client), "stale")  # apunta a otro servidor
            installer.register(client, [str(server)])
            server.unlink()
            self.assertEqual(installer.client_state(client), "stale")  # el servidor ya no existe
            path.write_text("{no es json", encoding="utf-8")
            self.assertEqual(installer.client_state(client), "missing")  # ilegible: no falla

    def test_client_state_reads_toml_too(self):
        server = self.home / "server.exe"
        server.write_text("exe", encoding="utf-8")
        (self.home / ".codex").mkdir()
        client = self.client("codex", "Codex", self.home / ".codex" / "config.toml", "toml")
        with patch.object(installer, "server_command", return_value=[str(server)]):
            self.assertEqual(installer.client_state(client), "missing")
            installer.register(client, [str(server)])
            self.assertEqual(installer.client_state(client), "ok")

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_saved_account_exposes_only_the_campus_never_the_key(self):
        self.assertEqual(installer.saved_account(), {})
        runtime.save_credentials("https://plugin.example", "PluginKey123")
        self.assertEqual(installer.saved_account(), {})  # la del plugin no es de este programa
        runtime.save_credentials("https://campus.example", "SecretKey123", protect=True)
        self.assertEqual(installer.saved_account(), {"url": "https://campus.example"})
        self.assertNotIn("SecretKey123", str(installer.saved_account()))

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_empty_key_reuses_the_saved_one_only_for_the_same_campus(self):
        runtime.save_credentials("https://campus.example", "PluginKey123")
        with self.assertRaises(installer.InstallError):  # la del plugin no se reutiliza
            installer.resolve_account("https://campus.example", "")
        runtime.save_credentials("https://campus.example", "SecretKey123", protect=True)
        self.assertEqual(installer.resolve_account("campus.example/", ""),
                         ("https://campus.example", "SecretKey123"))
        self.assertEqual(installer.resolve_account("https://campus.example", " NewKey1 "),
                         ("https://campus.example", "NewKey1"))
        with self.assertRaises(installer.InstallError):
            installer.resolve_account("https://otro.example", "")
        with patch.object(installer, "read_credentials", side_effect=ValueError), \
                self.assertRaises(installer.InstallError):
            installer.resolve_account("https://campus.example", "")

    def test_has_install_is_true_while_server_or_key_remain(self):
        self.assertFalse(installer.has_install())
        installer.installed_server().mkdir(parents=True)
        self.assertTrue(installer.has_install())

    def make_bundle(self, content="v1"):
        bundle = self.home / "bundle"
        (bundle / installer.SERVER_DIR / "_internal").mkdir(parents=True, exist_ok=True)
        (bundle / installer.SERVER_DIR / installer.SERVER_EXE).write_text(content, encoding="utf-8")
        (bundle / installer.SERVER_DIR / "_internal" / "base.dll").write_text("dll", encoding="utf-8")
        return patch.object(sys, "_MEIPASS", str(bundle), create=True)

    def test_half_deleted_server_is_detected_although_its_exe_remains(self):
        with self.make_bundle():
            client = self.client("claude-code", "Claude Code", self.home / ".claude.json", "json")
            installer.register(client, installer.install_server())
            self.assertEqual(installer.client_state(client), "ok")
            self.assertTrue(installer.server_intact())
            (installer.installed_server() / "_internal" / "base.dll").unlink()
            self.assertTrue((installer.installed_server() / installer.SERVER_EXE).is_file())
            self.assertFalse(installer.server_intact())
            self.assertEqual(installer.client_state(client), "stale")
            self.assertTrue(installer.snapshot()["broken"])
            installer.install_server()  # reparar
            self.assertEqual(installer.client_state(client), "ok")

    def test_old_installation_without_manifest_counts_as_broken(self):
        installer.installed_server().mkdir(parents=True)
        (installer.installed_server() / installer.SERVER_EXE).write_text("x", encoding="utf-8")
        self.assertTrue(installer.snapshot()["broken"])

    def test_locked_server_stays_complete_when_it_cannot_be_replaced(self):
        with self.make_bundle("v1"):
            installer.install_server()
        real_replace = os.replace

        def locked(source, target):
            if Path(source) == installer.installed_server():
                raise PermissionError("en uso")
            return real_replace(source, target)

        with self.make_bundle("v2"), patch("os.replace", side_effect=locked), patch("time.sleep"), \
                self.assertRaises(installer.InstallError):
            installer.install_server()
        self.assertEqual((installer.installed_server() / installer.SERVER_EXE).read_text(encoding="utf-8"), "v1")
        self.assertTrue(installer.server_intact())
        self.assertEqual(list(installer.installed_server().parent.iterdir()), [installer.installed_server()])

    def test_reinstalling_the_same_version_does_not_touch_a_server_in_use(self):
        with self.make_bundle():
            installer.install_server()
            self.assertTrue(installer.server_up_to_date())
            with patch("shutil.copytree") as copy, patch.object(installer, "running_servers", return_value=[7]):
                installer.install_server()
                self.assertEqual(installer.needs_closing_for_install([]), (False, False))
            copy.assert_not_called()
        with self.make_bundle("v2"), patch.object(installer, "running_servers", return_value=[7]):
            self.assertEqual(installer.needs_closing_for_install([]), (False, True))

    def test_only_our_server_processes_count_as_in_use(self):
        ours = str(installer.installed_server() / installer.SERVER_EXE)
        processes = {1: ours, 2: r"C:\otra\moodle-ai-connect-server.exe", 3: ours.replace("moodle-ai-connect-server\\", "moodle-ai-connect-server-x\\")}
        with patch.object(installer, "_process_paths", return_value=processes):
            self.assertEqual(installer.running_servers(), [1])

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_uninstall_with_server_in_use_changes_nothing(self):
        (self.home / ".claude.json").write_text("{}", encoding="utf-8")
        with self.make_bundle():
            client = installer.detect_clients()[0]
            installer.register(client, installer.install_server())
            runtime.save_credentials("https://campus.example", "SecretKey123", protect=True)
            with patch.object(installer, "running_servers", return_value=[7]), \
                    self.assertRaises(installer.InstallError):
                installer.uninstall()
            self.assertEqual(installer.client_state(client), "ok")
            self.assertTrue(runtime.protected_credentials_path().exists())

            real_replace = os.replace

            def locked(source, target):
                if Path(source) == installer.installed_server():
                    raise PermissionError("en uso")
                return real_replace(source, target)

            with patch("os.replace", side_effect=locked), patch("time.sleep"), \
                    self.assertRaises(installer.InstallError):
                installer.uninstall()
            self.assertEqual(installer.client_state(client), "ok")
            self.assertTrue(installer.server_intact())
            self.assertTrue(runtime.protected_credentials_path().exists())

    def test_uninstall_refuses_while_claude_desktop_has_the_entry_open(self):
        folder = self.home / "Roaming" / "Claude"
        folder.mkdir(parents=True)
        client = installer.detect_clients()[0]
        installer.register(client, COMMAND)
        with patch.object(installer, "running_claude_desktop", return_value=[100]), \
                self.assertRaises(installer.InstallError):
            installer.uninstall()
        self.assertTrue(installer.is_registered(client))
        # Abierto pero sin nuestra entrada, no hay nada que pueda reescribir.
        installer.unregister(client)
        with patch.object(installer, "running_claude_desktop", return_value=[100]):
            self.assertEqual(installer.needs_closing_for_uninstall(), (False, False))

    def test_uninstall_reports_a_client_it_could_not_clean(self):
        (self.home / ".codex").mkdir()
        (self.home / ".codex" / "config.toml").write_text('[mcp_servers.moodle-ai-connect]\ncommand = "x"\n[roto\n', encoding="utf-8")
        with self.assertRaises(installer.InstallError) as caught:
            installer.uninstall()
        self.assertIn("Codex", str(caught.exception))
        self.assertIn("[roto", (self.home / ".codex" / "config.toml").read_text(encoding="utf-8"))

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_install_uninstall_install_again_leaves_a_working_setup(self):
        (self.home / ".claude.json").write_text('{"numStartups": 7}', encoding="utf-8")
        with self.make_bundle():
            clients = installer.detect_clients()
            for _ in range(2):
                installer.install("https://campus.example", "SecretKey123", clients)
                self.assertEqual(installer.client_state(clients[0]), "ok")
                self.assertEqual(installer.saved_account(), {"url": "https://campus.example"})
                self.assertFalse(installer.snapshot()["broken"])
                self.assertEqual(installer.uninstall(), ["Claude Code"])
                state = installer.snapshot()
                self.assertFalse(state["installed"] or state["broken"])
                self.assertEqual(state["states"], {"claude-code": "missing"})
                self.assertEqual(state["account"], {})
                self.assertEqual(list(installer.installed_server().parent.iterdir()), [])
        self.assertEqual(json.loads((self.home / ".claude.json").read_text(encoding="utf-8"))["numStartups"], 7)

    def test_connection_is_tested_with_the_bundled_server_before_touching_anything(self):
        with self.make_bundle():
            self.assertTrue(installer.check_command()[0].startswith(str(self.home / "bundle")))
            self.assertFalse(installer.installed_server().exists())

    def test_check_survives_output_in_another_encoding(self):
        script = "import sys; sys.stderr.buffer.write(b'Moodle rechaz\\xf3 la clave'); sys.exit(1)"
        with self.assertRaises(installer.InstallError) as caught:
            installer.check_connection("https://campus.example", "abc123", [sys.executable, "-c", script])
        self.assertIn("Moodle rechaz", str(caught.exception))

    def test_verify_uses_the_saved_key_not_the_environment(self):
        completed = unittest.mock.Mock(returncode=0, stdout='{"user": "Ana"}', stderr="")
        with patch.dict(os.environ, {"MOODLE_TOKEN": "fromenv1"}), patch("subprocess.run", return_value=completed) as run:
            self.assertEqual(installer.verify_installed(), {"user": "Ana"})
        self.assertNotIn("MOODLE_TOKEN", run.call_args.kwargs["env"])

    def test_server_is_copied_on_first_install_and_replaced_on_update(self):
        bundle = self.home / "bundle"
        (bundle / installer.SERVER_DIR).mkdir(parents=True)
        executable = bundle / installer.SERVER_DIR / installer.SERVER_EXE
        executable.write_text("v1", encoding="utf-8")
        with patch.object(sys, "_MEIPASS", str(bundle), create=True):
            command = installer.install_server()
            self.assertEqual(Path(command[0]).read_text(encoding="utf-8"), "v1")
            executable.write_text("v2", encoding="utf-8")
            installer.install_server()
        self.assertEqual(Path(command[0]).read_text(encoding="utf-8"), "v2")
        self.assertEqual(list(installer.installed_server().parent.iterdir()), [installer.installed_server()])

    def test_campus_url_is_validated(self):
        self.assertEqual(installer.normalize_campus("sifo.comillas.edu/"), "https://sifo.comillas.edu")
        for bad in ("http://sifo.comillas.edu", "https://u:p@sifo.comillas.edu", "https://x.edu/?a=1", ""):
            with self.assertRaises(installer.InstallError):
                installer.normalize_campus(bad)

    def test_check_rejects_a_password_shaped_key_before_running_anything(self):
        with patch("subprocess.run") as run, self.assertRaises(installer.InstallError):
            installer.check_connection("https://sifo.comillas.edu", "mi clave!")
        run.assert_not_called()

    def test_check_passes_credentials_by_environment_not_arguments(self):
        completed = unittest.mock.Mock(returncode=0, stdout='{"user": "Ana"}', stderr="")
        with patch("subprocess.run", return_value=completed) as run:
            result = installer.check_connection("https://sifo.comillas.edu", "abc123", COMMAND)
        self.assertEqual(result, {"user": "Ana"})
        arguments, options = run.call_args
        self.assertNotIn("abc123", " ".join(arguments[0]))
        self.assertEqual(options["env"]["MOODLE_TOKEN"], "abc123")

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_key_is_encrypted_on_disk_and_readable_by_the_server(self):
        runtime.save_credentials("https://sifo.comillas.edu", "abc123XYZ", protect=True)
        self.assertNotIn("abc123XYZ", runtime.protected_credentials_path().read_text(encoding="utf-8"))
        self.assertFalse(runtime.credentials_path().exists())
        self.assertEqual(runtime.read_credentials(),
                         {"MOODLE_URL": "https://sifo.comillas.edu", "MOODLE_TOKEN": "abc123XYZ"})

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_uninstall_removes_entries_key_and_server(self):
        (self.home / ".codex").mkdir()
        client = installer.detect_clients()[0]
        runtime.save_credentials("https://sifo.comillas.edu", "abc123XYZ", protect=True)
        (installer.installed_server()).mkdir(parents=True)
        installer.register(client, COMMAND)
        self.assertEqual(installer.uninstall(), ["ChatGPT Work local / Codex"])
        self.assertFalse(runtime.protected_credentials_path().exists())
        self.assertFalse(installer.installed_server().exists())


if __name__ == "__main__":
    unittest.main()
