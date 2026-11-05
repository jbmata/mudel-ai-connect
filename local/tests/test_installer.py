import json
import os
import stat
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path
from contextlib import contextmanager
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(REPO / "local"), str(REPO / "shared")]

import mudel_ai_connect_installer as installer  # noqa: E402
import plugin_runtime as runtime  # noqa: E402

COMMAND = [r"C:\Users\ana\AppData\Local\mudel-ai-connect\bin\mudel-ai-connect-server\mudel-ai-connect-server.exe"]


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

    def test_new_claude_desktop_data_folder_wins_over_the_old_ones(self):
        old_store = self.home / "Local/Packages/Claude_abc/LocalCache/Roaming/Claude"
        old_store.mkdir(parents=True)
        (self.home / "Roaming/Claude").mkdir(parents=True)
        self.assertEqual([c.path.parent for c in installer.detect_clients()],
                         [self.home / "Roaming/Claude", old_store])
        new = self.home / "Local/Claude-Data/claude_abc"
        new.mkdir(parents=True)
        (self.home / "Local/Claude-Data/README.txt").write_text("aviso", encoding="utf-8")
        clients = installer.detect_clients()
        self.assertEqual([(c.key, c.path) for c in clients],
                         [("claude-desktop-0", new / "claude_desktop_config.json")])
        self.assertEqual(installer.claude_desktop_dirs()[1], [self.home / "Roaming/Claude", old_store])

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_install_writes_where_claude_desktop_reads_and_clears_the_old_place(self):
        old = self.home / "Local/Packages/Claude_abc/LocalCache/Roaming/Claude"
        old.mkdir(parents=True)
        stale = self.client("claude-desktop-0", "Claude Desktop", old / "claude_desktop_config.json", "json")
        installer.register(stale, COMMAND)  # lo que dejó la versión anterior del instalador
        new = self.home / "Local/Claude-Data/claude_abc"
        new.mkdir(parents=True)
        (new / "claude_desktop_config.json").write_text(
            '{"mcpServers": {}, "preferences": {"x": 1}}', encoding="utf-8")
        with self.make_bundle(), patch.object(installer, "running_claude_desktop", return_value=[]), \
                patch.object(installer, "verify_installed", return_value={}):
            installer.install("https://campus.example", "SecretKey123", installer.detect_clients())
            config = json.loads((new / "claude_desktop_config.json").read_text(encoding="utf-8"))
            self.assertIn(installer.SERVER_NAME, config["mcpServers"])
            self.assertEqual(config["preferences"], {"x": 1})
            self.assertFalse(installer.is_registered(stale))
            installer.uninstall()
        self.assertNotIn(installer.SERVER_NAME,
                         json.loads((new / "claude_desktop_config.json").read_text(encoding="utf-8"))["mcpServers"])

    def test_claude_code_inside_claude_desktop_is_not_claude_desktop(self):
        processes = {7: r"C:\Users\ana\AppData\Local\Claude-Data\claude_abc\claude-code\2.1.317\x\claude.exe",
                     8: r"C:\Program Files\WindowsApps\Claude_4.16336.0.0_x64__abc\app\claude.exe"}
        with patch.object(installer, "_process_paths", return_value=processes):
            self.assertEqual(installer.running_claude_desktop(), [8])

    def make_legacy_install(self):
        """Lo que deja en el equipo la app cuando se llamaba «Moodle AI Connect»."""
        runtime.save_credentials("https://campus.example", "LegacyKey123", protect=True)
        old = installer.legacy_dir()
        old.mkdir(parents=True)
        runtime.protected_credentials_path().rename(old / "credentials.dpapi.json")
        exe = old / "bin" / "moodle-ai-connect-server" / "moodle-ai-connect-server.exe"
        exe.parent.mkdir(parents=True)
        exe.write_text("exe antiguo", encoding="utf-8")
        (old / "runtime").mkdir()
        (old / "runtime" / "python.exe").write_text("entorno del plugin", encoding="utf-8")
        (old / "credentials.json").write_text('{"MOODLE_URL": "x", "MOODLE_TOKEN": "y"}', encoding="utf-8")
        entry = {"command": str(exe), "args": [], "env": {}}
        (self.home / ".claude.json").write_text(json.dumps(
            {"numStartups": 3, "mcpServers": {installer.LEGACY_NAME: entry, "otro": {"command": "x"}}}),
            encoding="utf-8")
        desktop = self.home / "Local/Claude-Data/claude_abc"
        desktop.mkdir(parents=True)
        (desktop / "claude_desktop_config.json").write_text(
            json.dumps({"mcpServers": {installer.LEGACY_NAME: entry}}), encoding="utf-8")
        (self.home / ".codex").mkdir()
        (self.home / ".codex/config.toml").write_text(
            f'model = "x"\n\n[mcp_servers.{installer.LEGACY_NAME}]\ncommand = "viejo"\n', encoding="utf-8")
        return old

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_previous_name_install_is_migrated_reusing_its_token(self):
        old = self.make_legacy_install()
        self.assertEqual(installer.saved_account(), {"url": "https://campus.example"})
        url, token = installer.resolve_account("https://campus.example", "")
        self.assertEqual(token, "LegacyKey123")
        clients = installer.detect_clients()
        with self.make_bundle(), patch.object(installer, "running_claude_desktop", return_value=[]), \
                patch.object(installer, "verify_installed", return_value={}):
            installer.install(url, token, clients)
            for client in clients:
                with self.subTest(client=client.name):
                    self.assertEqual(installer.client_state(client), "ok")
                    self.assertFalse(installer.is_registered(client, installer.LEGACY_NAME))
        self.assertEqual(json.loads((self.home / ".claude.json").read_text(encoding="utf-8"))["mcpServers"]["otro"],
                         {"command": "x"})
        self.assertFalse((old / "bin").exists())
        self.assertFalse((old / "credentials.dpapi.json").exists())
        self.assertTrue((old / "runtime" / "python.exe").is_file())  # el plugin por Python sigue intacto
        self.assertTrue((old / "credentials.json").is_file())
        self.assertEqual(runtime.read_credentials(protect=True)["MOODLE_TOKEN"], "LegacyKey123")

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_uninstall_also_removes_the_previous_name(self):
        old = self.make_legacy_install()
        (old / "runtime" / "python.exe").unlink()
        (old / "runtime").rmdir()
        (old / "credentials.json").unlink()
        with patch.object(installer, "running_claude_desktop", return_value=[]):
            installer.uninstall()
        for client in installer.detect_clients():
            with self.subTest(client=client.name):
                self.assertFalse(installer.is_registered(client, installer.LEGACY_NAME))
        self.assertFalse(old.exists())  # vacía: se borra entera

    def test_old_server_in_use_counts_as_ours(self):
        old_exe = installer.legacy_dir() / "bin" / "moodle-ai-connect-server" / "moodle-ai-connect-server.exe"
        processes = {1: str(old_exe), 2: r"C:\otra\moodle-ai-connect-server.exe"}
        with patch.object(installer, "_process_paths", return_value=processes):
            self.assertEqual(installer.running_servers(), [1])

    def test_codex_with_the_plugin_under_any_name_is_not_preselected(self):
        (self.home / ".codex").mkdir()
        for plugin in ("moodle@moodle-local", "moodle-ai-connect@moodle-ai-connect-local",
                       "mudel-ai-connect@mudel-ai-connect-local"):
            with self.subTest(plugin=plugin):
                (self.home / ".codex/config.toml").write_text(f'[plugins."{plugin}"]\nenabled = true\n',
                                                              encoding="utf-8")
                self.assertFalse(installer.detect_clients()[0].selected)

    def test_version_matches_the_plugin_manifests(self):
        local = REPO / "local"
        for manifest in (local / "plugin.json", local / ".claude-plugin" / "plugin.json"):
            with self.subTest(manifest=manifest.name):
                self.assertEqual(json.loads(manifest.read_text(encoding="utf-8"))["version"], installer.VERSION)

    def fake_releases(self, releases):
        response = unittest.mock.MagicMock()
        response.__enter__.return_value.read.return_value = json.dumps(releases).encode()
        return patch("urllib.request.urlopen", return_value=response)

    def test_newer_release_is_found_including_betas_and_ignoring_drafts(self):
        releases = [
            {"tag_name": "v0.3.0", "html_url": "https://github.com/jbmata/mcp-moodle/releases/tag/v0.3.0", "draft": True},
            {"tag_name": "v0.2.0-beta", "html_url": "https://github.com/jbmata/mcp-moodle/releases/tag/v0.2.0-beta",
             "prerelease": True},
            {"tag_name": "v0.1.0", "html_url": "https://github.com/jbmata/mcp-moodle/releases/tag/v0.1.0"},
            {"tag_name": "v9.9.9", "html_url": "https://malicioso.example/v9"},
        ]
        with self.fake_releases(releases), patch.object(installer, "VERSION", "0.1.0"):
            self.assertEqual(installer.newer_release()["version"], "v0.2.0-beta")
        with self.fake_releases(releases[2:3]), patch.object(installer, "VERSION", "0.1.0"):
            self.assertIsNone(installer.newer_release())  # ya es la última
        with self.fake_releases({"message": "Not Found"}):
            self.assertIsNone(installer.newer_release())  # repositorio privado o sin releases
        with patch("urllib.request.urlopen", side_effect=OSError("sin red")):
            self.assertIsNone(installer.newer_release())

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_diagnostics_never_contains_the_token(self):
        runtime.save_credentials("https://campus.example", "SecretToken987", protect=True)
        (self.home / ".claude.json").write_text("{}", encoding="utf-8")
        installer.log_path().write_text("--- error de prueba\n", encoding="utf-8")
        text = installer.diagnostics()
        self.assertIn(f"Mudel AI Connect {installer.VERSION}", text)
        self.assertIn("campus https://campus.example", text)
        self.assertIn("Claude Code: missing", text)
        self.assertIn("error de prueba", text)
        self.assertNotIn("SecretToken987", text)

    def test_json_registration_keeps_other_servers_and_settings(self):
        path = self.home / "claude_desktop_config.json"
        path.write_text(json.dumps({"theme": "dark", "mcpServers": {"otro": {"command": "x"}}}),
                        encoding="utf-8")
        client = self.client("claude-desktop-0", "Claude Desktop", path, "json")
        installer.register(client, COMMAND)
        config = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(config["theme"], "dark")
        self.assertEqual(config["mcpServers"]["otro"], {"command": "x"})
        self.assertEqual(config["mcpServers"]["mudel-ai-connect"], {"command": COMMAND[0], "args": [], "env": {}})
        self.assertTrue(path.with_name(path.name + ".mudel-ai-connect-backup").is_file())
        self.assertTrue(installer.unregister(client))
        self.assertEqual(json.loads(path.read_text(encoding="utf-8"))["mcpServers"], {"otro": {"command": "x"}})
        self.assertFalse(installer.unregister(client))

    def test_claude_code_entry_declares_stdio_and_missing_file_is_created(self):
        path = self.home / "nuevo" / "config.json"
        installer.register(self.client("claude-code", "Claude Code", path, "json"), COMMAND)
        entry = json.loads(path.read_text(encoding="utf-8"))["mcpServers"]["mudel-ai-connect"]
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
        path.write_text('model = "x"\n\n[mcp_servers.mudel-ai-connect]\ncommand = "viejo"\n\n'
                        '[mcp_servers.mudel-ai-connect.env]\nA = "1"\n\n[mcp_servers.mudel-ai-connect-otro]\ncommand = "keep"\n\n'
                        '[projects.p]\ntrust = true\n', encoding="utf-8")
        client = self.client("codex", "Codex", path, "toml")
        installer.register(client, COMMAND)
        installer.register(client, COMMAND)  # idempotente
        config = tomllib.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(config["model"], "x")
        self.assertEqual(config["mcp_servers"]["mudel-ai-connect"], {"command": COMMAND[0], "args": []})
        self.assertEqual(config["mcp_servers"]["mudel-ai-connect-otro"], {"command": "keep"})
        self.assertEqual(config["projects"]["p"], {"trust": True})
        self.assertEqual(path.read_text(encoding="utf-8").count("[mcp_servers.mudel-ai-connect]"), 1)
        self.assertTrue(installer.unregister(client))
        self.assertNotIn("mudel-ai-connect", tomllib.loads(path.read_text(encoding="utf-8"))["mcp_servers"])

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
            '[plugins."mudel-ai-connect@mudel-ai-connect-local"]\nenabled = true\n', encoding="utf-8")
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
        with patch.object(installer, "read_protected_credentials", side_effect=ValueError), \
                self.assertRaises(installer.InstallError):
            installer.resolve_account("https://campus.example", "")

    def test_has_install_is_true_while_server_or_key_remain(self):
        self.assertFalse(installer.has_install())
        installer.installed_server().mkdir(parents=True)
        self.assertTrue(installer.has_install())
        installer.installed_server().rmdir()
        (installer.installed_server().parent / (installer.SERVER_DIR + ".old-1")).mkdir()
        self.assertTrue(installer.has_install())

    @contextmanager
    def make_bundle(self, content="v1"):
        bundle = self.home / "bundle"
        (bundle / installer.SERVER_DIR / "_internal").mkdir(parents=True, exist_ok=True)
        (bundle / installer.SERVER_DIR / installer.SERVER_EXE).write_text(content, encoding="utf-8")
        (bundle / installer.SERVER_DIR / "_internal" / "base.dll").write_text("dll", encoding="utf-8")
        with patch.object(sys, "_MEIPASS", str(bundle), create=True), \
                patch.object(installer, "verify_installed", return_value={}):
            yield

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
        processes = {1: ours, 2: r"C:\otra\mudel-ai-connect-server.exe", 3: ours.replace("mudel-ai-connect-server\\", "mudel-ai-connect-server-x\\")}
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
        (self.home / ".codex" / "config.toml").write_text('[mcp_servers.mudel-ai-connect]\ncommand = "x"\n[roto\n', encoding="utf-8")
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

    def test_invalid_selected_configuration_is_checked_before_installing(self):
        valid = self.client("claude-code", "Claude Code", self.home / ".claude.json", "json")
        valid.path.write_text("{}", encoding="utf-8")
        invalid = self.client("codex", "Codex", self.home / "config.toml", "toml")
        invalid.path.write_text("[broken", encoding="utf-8")
        with patch.object(installer, "install_server") as copy, self.assertRaises(installer.InstallError):
            installer.install("https://campus.example", "NewToken123", [valid, invalid])
        copy.assert_not_called()
        self.assertFalse(runtime.protected_credentials_path().exists())

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_failed_update_restores_server_credentials_and_client_configuration(self):
        claude = self.client("claude-code", "Claude Code", self.home / ".claude.json", "json")
        claude.path.write_text("{}", encoding="utf-8")
        codex = self.client("codex", "Codex", self.home / "config.toml", "toml")
        codex.path.write_text('model = "original"\n', encoding="utf-8")
        with self.make_bundle():
            installer.install("https://old.example", "OldToken123", [claude, codex])
        config = json.loads(claude.path.read_text(encoding="utf-8"))
        config["mcpServers"][installer.SERVER_NAME]["env"] = {"CUSTOM": "preserve"}
        claude.path.write_text(json.dumps(config), encoding="utf-8")
        previous = {path: path.read_bytes() for path in (claude.path, codex.path, runtime.protected_credentials_path())}
        register = installer.register

        def fail_second_client(client, command):
            if client.key == "codex":
                raise OSError("No se pudo escribir el segundo cliente")
            register(client, command)

        with self.make_bundle("v2"), patch.object(installer, "register", side_effect=fail_second_client):
            with self.assertRaises((OSError, installer.InstallError)):
                installer.install("https://new.example", "NewToken123", [claude, codex])
        for path, content in previous.items():
            self.assertEqual(path.read_bytes(), content)
        self.assertEqual((installer.installed_server() / installer.SERVER_EXE).read_text(encoding="utf-8"), "v1")
        self.assertTrue(installer.server_intact())
        self.assertEqual(list(installer.installed_server().parent.iterdir()), [installer.installed_server()])

    def test_failed_first_install_removes_new_server_and_credentials(self):
        client = self.client("claude-code", "Claude Code", self.home / ".claude.json", "json")
        client.path.write_text("{}", encoding="utf-8")
        with self.make_bundle(), patch.object(installer, "register", side_effect=OSError("Error de escritura")):
            with self.assertRaises((OSError, installer.InstallError)):
                installer.install("https://campus.example", "NewToken123", [client])
        self.assertEqual(client.path.read_text(encoding="utf-8"), "{}")
        self.assertFalse(installer.installed_server().exists())
        self.assertFalse(runtime.protected_credentials_path().exists())
        self.assertFalse(runtime.credentials_path().exists())

    @unittest.skipUnless(sys.platform == "win32", "DPAPI solo existe en Windows")
    def test_failed_final_verification_restores_the_previous_installation(self):
        client = self.client("claude-code", "Claude Code", self.home / ".claude.json", "json")
        client.path.write_text("{}", encoding="utf-8")
        with self.make_bundle():
            installer.install("https://old.example", "OldToken123", [client])
        original_key = runtime.protected_credentials_path().read_bytes()
        with self.make_bundle("v2"), patch.object(installer, "verify_installed",
                side_effect=installer.InstallError("Falló la verificación final")):
            with self.assertRaisesRegex(installer.InstallError, "Falló la verificación final"):
                installer.install("https://new.example", "NewToken123", [client])
        self.assertEqual(runtime.protected_credentials_path().read_bytes(), original_key)
        self.assertEqual((installer.installed_server() / installer.SERVER_EXE).read_text(encoding="utf-8"), "v1")
        self.assertTrue(installer.server_intact())

    def test_quoted_toml_entries_can_be_updated_and_removed(self):
        client = self.client("codex", "Codex", self.home / "config.toml", "toml")
        for header in ("mcp_servers.'mudel-ai-connect'", '"mcp_servers"."mudel-ai-connect"',
                       "'mcp_servers'.'mudel-ai-connect'"):
            with self.subTest(header=header):
                client.path.write_text(f'model = "keep"\n[{header}]\ncommand = "old"\n', encoding="utf-8")
                installer.register(client, COMMAND)
                self.assertEqual(tomllib.loads(client.path.read_text(encoding="utf-8"))["mcp_servers"][installer.SERVER_NAME]["command"], COMMAND[0])
                self.assertTrue(installer.unregister(client))
                self.assertFalse(installer.is_registered(client))
                self.assertEqual(tomllib.loads(client.path.read_text(encoding="utf-8"))["model"], "keep")

    def test_previous_bundle_is_reported_as_outdated(self):
        client = self.client("claude-code", "Claude Code", self.home / ".claude.json", "json")
        client.path.write_text("{}", encoding="utf-8")
        with self.make_bundle():
            installer.register(client, installer.install_server())
        with self.make_bundle("v2"):
            self.assertFalse(installer.server_up_to_date())
            self.assertEqual(installer.client_state(client), "stale")
            self.assertEqual(installer.snapshot()["states"][client.key], "stale")

    @unittest.skipUnless(sys.platform == "win32", "Atributo de solo lectura de Windows")
    def test_read_only_server_files_are_removed_on_uninstall(self):
        (self.home / ".claude.json").write_text("{}", encoding="utf-8")
        with self.make_bundle():
            installer.install("https://campus.example", "TestToken123", installer.detect_clients())
            (installer.installed_server() / "_internal/base.dll").chmod(stat.S_IREAD)
            try:
                installer.uninstall()
                self.assertEqual(list(installer.installed_server().parent.iterdir()), [])
                self.assertFalse(installer.has_install())
            finally:
                for path in self.home.rglob("*"):
                    if path.is_file():
                        path.chmod(stat.S_IREAD | stat.S_IWRITE)

    def test_failed_cleanup_is_reported_and_can_be_retried(self):
        (self.home / ".claude.json").write_text("{}", encoding="utf-8")
        with self.make_bundle():
            installer.install("https://campus.example", "TestToken123", installer.detect_clients())
            with patch("shutil.rmtree", side_effect=PermissionError("Bloqueado")), patch("time.sleep"):
                with self.assertRaises(installer.InstallError):
                    installer.uninstall()
            self.assertTrue(installer.has_install())
            self.assertTrue(installer.is_registered(installer.detect_clients()[0]))
            installer.uninstall()
            self.assertFalse(installer.has_install())
            self.assertEqual(list(installer.installed_server().parent.iterdir()), [])

    def test_bundle_manifest_does_not_include_itself(self):
        with self.make_bundle():
            source = installer.bundled_server()
            installer.write_server_manifest(source)
            self.assertNotIn(installer.MANIFEST, json.loads((source / installer.MANIFEST).read_text(encoding="utf-8"))["files"])
            installer.install_server()
            self.assertTrue(installer.server_up_to_date())
            self.assertTrue(installer.server_intact())
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
