import json
import os
import sys
import tempfile
import tkinter as tk
import unittest
from pathlib import Path
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(REPO / "local"), str(REPO / "shared")]

import mudel_ai_connect_installer as installer  # noqa: E402
import mudel_ai_connect_setup  # noqa: E402
import plugin_runtime as runtime  # noqa: E402


class SetupWindowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        environment = {"USERPROFILE": str(self.home), "APPDATA": str(self.home / "Roaming"),
                       "LOCALAPPDATA": str(self.home / "Local")}
        patcher = patch.dict(os.environ, environment)
        patcher.start()
        self.addCleanup(patcher.stop)
        (self.home / ".claude.json").write_text("{}", encoding="utf-8")
        try:
            self.root = tk.Tk()
        except tk.TclError:
            self.skipTest("No hay pantalla disponible para tkinter.")
        self.root.withdraw()
        self.addCleanup(self.root.destroy)

    def test_window_reflects_the_disk_and_updates_after_installing(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        self.assertEqual(window.banner.cget("text"), "Sin instalar")
        self.assertEqual(window.connect.cget("text"), "Instalar")
        self.assertEqual(str(window.remove.cget("state")), "disabled")
        self.assertEqual(window.badges["claude-code"].cget("text"), "Sin conectar")
        self.assertEqual(window.url.get(), installer.DEFAULT_CAMPUS)

        server = self.home / "server.exe"
        server.write_text("exe", encoding="utf-8")
        client = installer.detect_clients()[0]
        with patch.object(installer, "server_command", return_value=[str(server)]):
            installer.register(client, [str(server)])
            runtime.save_credentials("https://campus.example", "SavedKey123", protect=True)
            window.url.set("https://campus.example")
            window.refresh()
            self.assertEqual(window.banner.cget("text"), "✓ Todo listo")
            self.assertEqual(window.badges["claude-code"].cget("text"), "✓ Conectado")
            self.assertEqual(window.connect.cget("text"), "Reinstalar")
            self.assertEqual(str(window.remove.cget("state")), "normal")
            self.assertIn("guardado", window.key_label.cget("text"))

            installer.register(client, [str(self.home / "otro.exe")])
            window.refresh()
            self.assertEqual(window.banner.cget("text"), "Actualización pendiente")
            self.assertEqual(window.connect.cget("text"), "Actualizar")
            self.assertEqual(window.badges["claude-code"].cget("text"), "Actualizar")

            installer.installed_server().mkdir(parents=True)  # sin manifiesto: incompleta
            window.refresh()
            self.assertEqual(window.banner.cget("text"), "Instalación incompleta")
            self.assertEqual(window.connect.cget("text"), "Reparar")

    def test_button_always_responds_and_says_what_is_missing(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        self.assertEqual(str(window.connect.cget("state")), "normal")
        window.connect.invoke()  # sin clave
        self.assertIn("Introduce tu token", window.log.get("1.0", "end"))
        window.token.set("mi contraseña!")
        self.assertIn("solo letras", window.key_label.cget("text"))
        window.token.set("abc123")
        window.url.set("http://inseguro.example")
        window.connect.invoke()
        self.assertIn("HTTPS", window.log.get("1.0", "end"))
        window.url.set(installer.DEFAULT_CAMPUS)
        window.rows["claude-code"].set(False)
        window.connect.invoke()
        self.assertIn("Marca al menos", window.log.get("1.0", "end"))
        self.assertFalse(window.working)

    def test_window_fits_small_laptop_screens_with_results_and_help_open(self):
        for screen in (768,):  # p. ej. 1366 × 768; aquí con el escalado del equipo, más exigente
            with self.subTest(screen=screen), patch.object(self.root, "winfo_screenheight", return_value=screen):
                window = mudel_ai_connect_setup.SetupWindow(self.root)
                self.root.deiconify()
                window.token_help_toggle.invoke()
                for number in range(9):
                    window.step(f"Paso {number}")
                self.root.update()
                bottom = self.root.winfo_y() + self.root.winfo_reqheight() + mudel_ai_connect_setup.TITLE_BAR
                self.assertLessEqual(bottom, screen - mudel_ai_connect_setup.TASKBAR)
                self.assertGreaterEqual(int(window.log.cget("height")), 2)
                self.assertIn("Paso 8", window.log.get("1.0", "end"))
                self.assertEqual(window.token_help.winfo_manager(), "")  # la ayuda se plegó para hacer sitio
                for child in self.root.winfo_children():
                    child.destroy()

    def test_window_stays_above_the_taskbar_when_results_appear(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        self.root.deiconify()
        window.token_help_toggle.invoke()
        self.root.geometry(f"+0+{self.root.winfo_screenheight() - 200}")
        self.root.update()
        for number in range(8):
            window.step(f"Paso {number}")
        self.root.update()
        bottom = self.root.winfo_y() + self.root.winfo_reqheight() + mudel_ai_connect_setup.TITLE_BAR
        self.assertLessEqual(bottom, self.root.winfo_screenheight() - mudel_ai_connect_setup.TASKBAR)

    def test_steps_show_progress_and_stop_at_the_failure(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        self.assertEqual(window.log.winfo_manager(), "")  # sin nada que contar, oculto
        window.step("Probando")
        window.step("Instalando")
        window.fail("Clave incorrecta")
        text = window.log.get("1.0", "end")
        self.assertIn("✓  Probando", text)
        self.assertIn("✗  Instalando", text)
        self.assertIn("✗  Clave incorrecta", text)

    def test_monitor_applies_external_changes_but_not_during_an_operation(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        client = installer.detect_clients()[0]
        installer.register(client, [str(self.home / "otro.exe")])
        snap = installer.snapshot()
        window.working = True
        window.handle("snapshot", snap)
        self.assertEqual(window.badges["claude-code"].cget("text"), "Sin conectar")
        window.working = False
        window.handle("snapshot", snap)
        self.assertEqual(window.badges["claude-code"].cget("text"), "Actualizar")

    def test_token_instructions_expand_without_opening_a_browser_or_changing_inputs(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        window.url.set("https://campus.example")
        window.token.set("ExampleToken123")
        self.root.update_idletasks()
        collapsed_height = self.root.winfo_reqheight()
        self.assertEqual(window.token_help.winfo_manager(), "")
        with patch("webbrowser.open") as browser:
            window.token_help_toggle.invoke()
            self.assertEqual(window.token_help.winfo_manager(), "grid")
            self.assertGreater(self.root.winfo_reqheight(), collapsed_height)
            steps = window.token_help.cget("text")
            for expected in ("Preferencias", "Claves de seguridad", "Servicios web", "RSS", "soporte"):
                self.assertIn(expected, steps)
            self.assertEqual(window.url.get(), "https://campus.example")
            self.assertEqual(window.token.get(), "ExampleToken123")
            window.token_help_toggle.invoke()
            self.assertEqual(window.token_help.winfo_manager(), "")
            self.assertEqual(self.root.winfo_reqheight(), collapsed_height)
            browser.assert_not_called()

    def test_chatgpt_and_codex_share_one_selection_and_connection_state(self):
        (self.home / ".codex").mkdir()
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        labels = [widget.cget("text") for row in window.picks.winfo_children()
                  for widget in row.winfo_children() if widget.winfo_class() == "Checkbutton"]
        self.assertIn("ChatGPT Work local / Codex", labels)
        self.assertTrue(window.rows["codex"].get())
        server = self.home / "server.exe"
        server.write_text("exe", encoding="utf-8")
        client = next(client for client in installer.detect_clients() if client.key == "codex")
        with patch.object(installer, "server_command", return_value=[str(server)]):
            installer.register(client, [str(server)])
            window.refresh()
        self.assertEqual(window.badges["codex"].cget("text"), "✓ Conectado")
        self.assertIn("Work local", client.note)

    def test_footer_shows_version_copies_diagnostics_and_offers_updates(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        labels = [w.cget("text") for f in window.root.winfo_children()[0].winfo_children()
                  if f.winfo_class() == "Frame" for w in f.winfo_children() if w.winfo_class() == "Label"]
        self.assertTrue(any(text.startswith(f"v{installer.VERSION}") and "No oficial" in text for text in labels))
        window.copy_diagnostics()
        self.assertIn(f"Mudel AI Connect {installer.VERSION}", self.root.clipboard_get())
        self.assertIn("nunca tu token", window.log.get("1.0", "end"))
        window.handle("update", None)
        self.assertEqual(window.update_link.winfo_manager(), "")
        window.handle("update", {"version": "v0.2.0", "url": "https://github.com/jbmata/mudel-ai-connect/releases/tag/v0.2.0"})
        self.assertEqual(window.update_link.winfo_manager(), "pack")
        self.assertIn("v0.2.0", window.update_link.cget("text"))

    def test_old_result_disappears_when_the_machine_changes(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        window.mode = "install"
        window.busy(True, "Instalando…")
        window.handle("done", [])
        self.assertIn("Listo", window.log.get("1.0", "end"))
        window.refresh()  # sin cambios: el resultado sigue a la vista
        self.assertEqual(window.log.winfo_manager(), "grid")
        installer.register(installer.detect_clients()[0], [str(self.home / "otro.exe")])
        window.refresh()
        self.assertEqual(window.log.winfo_manager(), "")

    def test_uninstalling_resets_the_window(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        window.mode = "uninstall"
        window.busy(True, "Desinstalando…")
        window.handle("done", ["Claude Code"])
        self.assertFalse(window.working)
        self.assertEqual(window.banner.cget("text"), "Sin instalar")
        self.assertEqual(window.connect.cget("text"), "Instalar")
        self.assertIn("Desinstalado de Claude Code", window.log.get("1.0", "end"))

    def test_install_without_a_key_or_saved_account_asks_for_it(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        with patch.object(installer, "install") as install:
            window.start_install()
        install.assert_not_called()
        self.assertIn("token", window.log.get("1.0", "end"))


    def test_permissions_can_be_saved_without_a_token_and_are_loaded_on_reopen(self):
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        window.permissions_button.invoke()
        dialog = next(child for child in self.root.winfo_children() if isinstance(child, tk.Toplevel))
        panel = dialog.winfo_children()[0]
        calendar = next(child for child in panel.winfo_children()
                        if isinstance(child, tk.Checkbutton) and child.cget("text") == "Calendario")
        calendar.invoke()
        actions = panel.winfo_children()[-1]
        actions.winfo_children()[0].invoke()
        self.assertEqual(runtime.read_permissions(), {**dict.fromkeys(runtime.PERMISSIONS, True), "calendar": False})
        self.assertFalse(runtime.protected_credentials_path().exists())
        self.assertIn("Permisos guardados", window.log.get("1.0", "end"))
        window.permissions_button.invoke()
        reopened = next(child for child in self.root.winfo_children() if isinstance(child, tk.Toplevel))
        calendar = next(child for child in reopened.winfo_children()[0].winfo_children()
                        if isinstance(child, tk.Checkbutton) and child.cget("text") == "Calendario")
        self.assertFalse(bool(reopened.getvar(calendar.cget("variable"))))

    def test_download_folder_is_chosen_by_the_user_and_cancel_keeps_the_previous_one(self):
        chosen = self.home / "Universidad"
        chosen.mkdir()
        window = mudel_ai_connect_setup.SetupWindow(self.root)

        def open_dialog():
            window.permissions_button.invoke()
            dialog = [child for child in self.root.winfo_children() if isinstance(child, tk.Toplevel)][-1]
            panel = dialog.winfo_children()[0]
            labels = [widget.cget("text") for frame in panel.winfo_children() if isinstance(frame, tk.Frame)
                      for widget in frame.winfo_children() if isinstance(widget, tk.Label)]
            return dialog, panel, next(text for text in labels if text.startswith("Se guardan en"))

        dialog, panel, shown = open_dialog()
        self.assertIn("Mudel AI Connect", shown)
        with patch.object(mudel_ai_connect_setup.filedialog, "askdirectory", return_value=str(chosen)):
            window.download_folder_button.invoke()
        panel.winfo_children()[-1].winfo_children()[0].invoke()  # Guardar
        self.assertEqual(runtime.read_download_folder(), chosen)

        dialog, panel, shown = open_dialog()
        self.assertIn(str(chosen), shown)
        with patch.object(mudel_ai_connect_setup.filedialog, "askdirectory", return_value=str(self.home)):
            window.download_folder_button.invoke()
        panel.winfo_children()[-1].winfo_children()[1].invoke()  # Cancelar
        self.assertEqual(runtime.read_download_folder(), chosen)

    def test_invalid_permissions_open_with_all_queries_disabled_and_cancel_preserves_them(self):
        folder = runtime.data_dir()
        folder.mkdir(parents=True)
        policy = folder / "permissions.json"
        policy.write_text("invalid", encoding="utf-8")
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        window.permissions_button.invoke()
        dialog = next(child for child in self.root.winfo_children() if isinstance(child, tk.Toplevel))
        panel = dialog.winfo_children()[0]
        checks = [child for child in panel.winfo_children() if isinstance(child, tk.Checkbutton)]
        self.assertEqual(len(checks), len(runtime.PERMISSIONS))
        self.assertTrue(all(not bool(dialog.getvar(child.cget("variable"))) for child in checks))
        panel.winfo_children()[-1].winfo_children()[1].invoke()
        self.assertEqual(policy.read_text(encoding="utf-8"), "invalid")

    def test_malformed_saved_key_shows_an_error_instead_of_a_callback_exception(self):
        runtime.data_dir().mkdir(parents=True)
        runtime.protected_credentials_path().write_text(json.dumps({
            "MOODLE_URL": installer.DEFAULT_CAMPUS, "MOODLE_TOKEN_DPAPI": None,
        }), encoding="utf-8")
        window = mudel_ai_connect_setup.SetupWindow(self.root)
        errors = []
        self.root.report_callback_exception = lambda *arguments: errors.append(arguments[0])
        window.connect.invoke()
        self.assertEqual(errors, [])
        self.assertIn("Introduce tu token", window.log.get("1.0", "end"))

    def test_old_server_shows_update_and_permission_changes_warn_about_it(self):
        bundle = self.home / "bundle"
        server = bundle / installer.SERVER_DIR / installer.SERVER_EXE
        server.parent.mkdir(parents=True)
        server.write_text("v1", encoding="utf-8")
        with patch.object(sys, "_MEIPASS", str(bundle), create=True):
            client = installer.detect_clients()[0]
            installer.register(client, installer.install_server())
            server.write_text("v2", encoding="utf-8")
            window = mudel_ai_connect_setup.SetupWindow(self.root)
            self.assertEqual(window.banner.cget("text"), "Actualización pendiente")
            self.assertEqual(window.connect.cget("text"), "Actualizar")
            window.permissions_button.invoke()
            dialog = next(child for child in self.root.winfo_children() if isinstance(child, tk.Toplevel))
            dialog.winfo_children()[0].winfo_children()[-1].winfo_children()[0].invoke()
            self.assertIn("Actualiza", window.log.get("1.0", "end"))


if __name__ == "__main__":
    unittest.main()
