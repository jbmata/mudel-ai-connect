import os
import sys
import tempfile
import tkinter as tk
import unittest
from pathlib import Path
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(REPO / "local"), str(REPO / "shared")]

import moodle_ai_connect_installer as installer  # noqa: E402
import moodle_ai_connect_setup  # noqa: E402
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
        window = moodle_ai_connect_setup.SetupWindow(self.root)
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
        window = moodle_ai_connect_setup.SetupWindow(self.root)
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

    def test_window_stays_above_the_taskbar_when_results_appear(self):
        window = moodle_ai_connect_setup.SetupWindow(self.root)
        self.root.deiconify()
        window.token_help_toggle.invoke()
        self.root.geometry(f"+0+{self.root.winfo_screenheight() - 200}")
        self.root.update()
        for number in range(8):
            window.step(f"Paso {number}")
        self.root.update()
        bottom = self.root.winfo_y() + self.root.winfo_reqheight() + moodle_ai_connect_setup.TITLE_BAR
        self.assertLessEqual(bottom, self.root.winfo_screenheight() - moodle_ai_connect_setup.TASKBAR)

    def test_steps_show_progress_and_stop_at_the_failure(self):
        window = moodle_ai_connect_setup.SetupWindow(self.root)
        self.assertEqual(window.log.winfo_manager(), "")  # sin nada que contar, oculto
        window.step("Probando")
        window.step("Instalando")
        window.fail("Clave incorrecta")
        text = window.log.get("1.0", "end")
        self.assertIn("✓  Probando", text)
        self.assertIn("✗  Instalando", text)
        self.assertIn("✗  Clave incorrecta", text)

    def test_monitor_applies_external_changes_but_not_during_an_operation(self):
        window = moodle_ai_connect_setup.SetupWindow(self.root)
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
        window = moodle_ai_connect_setup.SetupWindow(self.root)
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
        window = moodle_ai_connect_setup.SetupWindow(self.root)
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

    def test_old_result_disappears_when_the_machine_changes(self):
        window = moodle_ai_connect_setup.SetupWindow(self.root)
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
        window = moodle_ai_connect_setup.SetupWindow(self.root)
        window.mode = "uninstall"
        window.busy(True, "Desinstalando…")
        window.handle("done", ["Claude Code"])
        self.assertFalse(window.working)
        self.assertEqual(window.banner.cget("text"), "Sin instalar")
        self.assertEqual(window.connect.cget("text"), "Instalar")
        self.assertIn("Desinstalado de Claude Code", window.log.get("1.0", "end"))

    def test_install_without_a_key_or_saved_account_asks_for_it(self):
        window = moodle_ai_connect_setup.SetupWindow(self.root)
        with patch.object(installer, "install") as install:
            window.start_install()
        install.assert_not_called()
        self.assertIn("token", window.log.get("1.0", "end"))


if __name__ == "__main__":
    unittest.main()
