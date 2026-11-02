"""Ventana de Moodle AI Connect: conecta Moodle con las aplicaciones de IA de este equipo."""

from __future__ import annotations

import queue
import sys
import threading
import tkinter as tk
from pathlib import Path
from tkinter import messagebox, ttk

import moodle_ai_connect_installer as installer

# Paleta del logo: trazo casi negro, fondo blanco y un único acento rosa.
INK, PINK, PAPER, SOFT, MUTED, OFF = "#191A1E", "#F9017C", "#FFFFFF", "#F4F4F5", "#6B6B73", "#B8B8BE"
FONT = "Segoe UI"
STATES = {"ok": ("✓ Conectado", INK), "stale": ("Actualizar", PINK), "missing": ("Sin conectar", MUTED)}
MARKS = {"run": ("…  ", ""), "ok": ("✓  ", ""), "err": ("✗  ", "err"), "note": ("    ", "muted"), "done": ("✓  ", "ok")}
REFRESH_SECONDS = 3
TASKBAR, TITLE_BAR = 70, 45


def asset(name: str) -> Path:
    return Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent)) / "assets" / name


class SetupWindow:
    def __init__(self, root: tk.Tk) -> None:
        self.root = root
        self.events: queue.Queue = queue.Queue()
        self.wake, self.closing = threading.Event(), threading.Event()
        self.mode = "install"
        self.working = False
        self.can_remove = False
        self.snap: dict | None = None
        self.signature: tuple | None = None
        self.clients: list[installer.Client] | None = None
        self.rows: dict[str, tk.BooleanVar] = {}
        self.badges: dict[str, tk.Label] = {}
        self.notes: dict[str, tk.Label] = {}
        self.steps: list[list[str]] = []
        root.title("Moodle AI Connect")
        root.configure(background=PAPER)
        root.resizable(False, False)
        root.report_callback_exception = lambda *_args: installer.log_error()
        root.protocol("WM_DELETE_WINDOW", self.on_close)
        try:
            self.logo = tk.PhotoImage(file=str(asset("logo-setup.png")))
            root.iconphoto(True, self.logo)
        except tk.TclError:
            self.logo = None

        page = tk.Frame(root, background=PAPER, padx=28, pady=16)
        page.grid()
        page.columnconfigure(0, weight=1)

        header = tk.Frame(page, background=PAPER)
        header.grid(row=0, column=0, sticky="we")
        if self.logo is not None:
            tk.Label(header, image=self.logo, background=PAPER).pack(side="left", padx=(0, 18))
        titles = tk.Frame(header, background=PAPER)
        titles.pack(side="left", anchor="center")
        tk.Label(titles, text="Moodle AI Connect", font=(FONT, 22, "bold"), foreground=INK,
                 background=PAPER).pack(anchor="w")
        tk.Label(titles, text="Conecta tus asignaturas con tu IA.",
                 font=(FONT, 11), foreground=MUTED, background=PAPER, justify="left").pack(anchor="w", pady=(4, 0))
        self.banner = tk.Label(titles, font=(FONT, 11, "bold"), background=PAPER, anchor="w")
        self.banner.pack(anchor="w", pady=(10, 0))

        self.field(page, 1, "DIRECCIÓN DE MOODLE")
        self.url = tk.StringVar(value=installer.saved_account().get("url", installer.DEFAULT_CAMPUS))
        self.entry(page, 2, self.url)

        self.key_label = self.field(page, 3, "")
        self.token = tk.StringVar()
        self.key_entry = self.entry(page, 4, self.token, secret=True)
        help_area = tk.Frame(page, background=PAPER)
        help_area.grid(row=5, column=0, sticky="we", pady=(6, 0))
        help_area.columnconfigure(0, weight=1)
        self.token_help_toggle = tk.Button(
            help_area, text="▸ Cómo obtener el token", command=self.toggle_token_help,
            foreground=PINK, background=PAPER, activeforeground=INK, activebackground=PAPER,
            font=(FONT, 10, "bold"), cursor="hand2", relief="flat", bd=0, padx=0,
            highlightthickness=1, highlightbackground=PAPER, highlightcolor=PINK, anchor="w")
        self.token_help_toggle.grid(row=0, column=0, sticky="w")
        self.token_help = tk.Label(
            help_area, text=(
                "1. Entra en Moodle con tu cuenta.\n"
                "2. Abre tu foto o nombre → Preferencias.\n"
                "3. En «Cuenta de usuario», abre «Claves de seguridad».\n"
                "4. En «Servicios web», copia tu token (no el de RSS).\n"
                "5. Pégalo en el campo de arriba.\n\n"
                "Si no aparece esa opción o no hay un token, pídelo al soporte de tu centro."),
            wraplength=496, justify="left", anchor="w", font=(FONT, 9),
            foreground=INK, background=SOFT, padx=12, pady=10)
        self.token_help.grid(row=1, column=0, sticky="we", pady=(4, 0))
        self.token_help.grid_remove()

        self.field(page, 6, "CONECTAR CON")
        self.picks = tk.Frame(page, background=PAPER)
        self.picks.grid(row=7, column=0, sticky="we")

        tk.Label(page, wraplength=520, justify="left", font=(FONT, 9), foreground=MUTED,
                 background=PAPER, text=(
                     "El token de autenticación se guarda cifrado en tu PC "
                     "y solo se envía a tu Moodle.")).grid(row=8, column=0, sticky="w", pady=(12, 0))

        actions = tk.Frame(page, background=PAPER)
        actions.grid(row=9, column=0, sticky="we", pady=(12, 0))
        self.connect = tk.Button(actions, text="Instalar", command=self.start_install,
                                 font=(FONT, 11, "bold"), foreground=PAPER, background=INK,
                                 activeforeground=PAPER, activebackground=PINK, relief="flat", bd=0,
                                 padx=22, pady=9, cursor="hand2", disabledforeground=PAPER)
        self.connect.pack(side="left")
        self.connect.bind("<Enter>", lambda _e: str(self.connect["state"]) != "disabled"
                          and self.connect.configure(background=PINK))
        self.connect.bind("<Leave>", lambda _e: self.connect.configure(
            background=OFF if str(self.connect["state"]) == "disabled" else INK))
        outline = tk.Frame(actions, background=INK, padx=2, pady=2)
        outline.pack(side="left", padx=12)
        self.outline = outline
        self.remove = tk.Button(outline, text="Desinstalar", command=self.start_uninstall,
                                font=(FONT, 11), foreground=INK, background=PAPER,
                                activeforeground=PINK, activebackground=PAPER, relief="flat",
                                bd=0, padx=16, pady=7, cursor="hand2", disabledforeground=OFF)
        self.remove.pack()
        self.remove.bind("<Enter>", lambda _e: str(self.remove["state"]) != "disabled" and (
            self.remove.configure(foreground=PINK), outline.configure(background=PINK)))
        self.remove.bind("<Leave>", lambda _e: (self.remove.configure(foreground=INK), self.paint_remove()))

        style = ttk.Style(root)
        style.theme_use("clam")
        style.configure("Moodle.Horizontal.TProgressbar", troughcolor=SOFT, background=PINK,
                        bordercolor=SOFT, lightcolor=PINK, darkcolor=PINK, thickness=5)
        self.progress = ttk.Progressbar(page, mode="indeterminate", style="Moodle.Horizontal.TProgressbar")
        self.progress.grid(row=10, column=0, sticky="we", pady=(12, 0))
        self.progress.grid_remove()

        self.log = tk.Text(page, height=6, width=58, state="disabled", wrap="word", relief="flat",
                           background=SOFT, foreground=INK, font=(FONT, 10), padx=12, pady=10,
                           highlightthickness=0, cursor="arrow")
        self.log.grid(row=11, column=0, sticky="we", pady=(10, 0))
        self.log.grid_remove()  # Sin nada que contar no ocupa sitio.
        self.log.tag_configure("ok", font=(FONT, 10, "bold"))
        self.log.tag_configure("err", foreground=PINK, font=(FONT, 10, "bold"))
        self.log.tag_configure("muted", foreground=MUTED)

        self.url.trace_add("write", lambda *_args: self.validate())
        self.token.trace_add("write", lambda *_args: self.validate())
        self.refresh()
        self.root.after(100, self.poll)

    @staticmethod
    def field(page: tk.Frame, row: int, text: str) -> tk.Label:
        label = tk.Label(page, text=text, font=(FONT, 8, "bold"), foreground=MUTED, background=PAPER)
        label.grid(row=row, column=0, sticky="w", pady=(12, 3))
        return label

    @staticmethod
    def entry(page: tk.Frame, row: int, variable: tk.StringVar, secret: bool = False) -> tk.Entry:
        widget = tk.Entry(page, textvariable=variable, show="•" if secret else "", font=(FONT, 12),
                          foreground=INK, background=PAPER, insertbackground=INK, relief="flat",
                          highlightthickness=2, highlightbackground=INK, highlightcolor=PINK)
        widget.grid(row=row, column=0, sticky="we", ipady=5)
        return widget

    def start_monitor(self) -> None:
        """Relee el estado cada pocos segundos, sin bloquear la ventana."""
        def loop() -> None:
            while not self.closing.is_set():
                self.wake.wait(REFRESH_SECONDS)
                self.wake.clear()
                if self.closing.is_set():
                    return
                if not self.working:
                    try:
                        self.events.put(("snapshot", installer.snapshot()))
                    except Exception:
                        installer.log_error()

        # Al volver a la ventana no se espera al siguiente ciclo.
        self.root.bind("<FocusIn>", lambda _event: self.wake.set())
        threading.Thread(target=loop, daemon=True).start()

    def on_close(self) -> None:
        if self.working:
            return  # Cerrar a mitad dejaría la operación sin terminar.
        self.closing.set()
        self.wake.set()
        self.root.destroy()

    def build_rows(self, clients: list[installer.Client], states: dict[str, str]) -> None:
        for child in self.picks.winfo_children():
            child.destroy()
        self.rows, self.badges, self.notes = {}, {}, {}
        if not clients:
            tk.Label(self.picks, text="No hay aplicaciones compatibles.\n"
                     "Instala y abre Claude, Codex o ChatGPT Work local.", wraplength=520, font=(FONT, 10), foreground=INK,
                     background=PAPER, justify="left").pack(anchor="w")
            return
        # Se marca lo que falta; si todo está conectado, se deja marcado para poder reinstalar.
        pending = [c.key for c in clients if c.selected and states[c.key] != "ok"]
        ticked = pending or [c.key for c in clients if c.selected]
        for client in clients:
            row = tk.Frame(self.picks, background=PAPER)
            row.pack(fill="x", pady=1)
            variable = tk.BooleanVar(value=client.key in ticked)
            variable.trace_add("write", lambda *_args: self.validate())
            self.rows[client.key] = variable
            tk.Checkbutton(row, text=client.name, variable=variable, font=(FONT, 11), foreground=INK,
                           background=PAPER, activebackground=PAPER, activeforeground=PINK,
                           selectcolor=PAPER, anchor="w", highlightthickness=0, cursor="hand2").pack(side="left")
            self.notes[client.key] = tk.Label(row, font=(FONT, 9), foreground=MUTED, background=PAPER)
            self.notes[client.key].pack(side="left", padx=(6, 0))
            self.badges[client.key] = tk.Label(row, font=(FONT, 10, "bold"), background=PAPER)
            self.badges[client.key].pack(side="right")

    def refresh(self) -> None:
        self.apply(installer.snapshot())

    def apply(self, snap: dict) -> None:
        """Pinta lo que hay en el equipo: estados, resumen y botones."""
        self.snap = snap
        clients, states = snap["clients"], snap["states"]
        signature = (snap["broken"], tuple(sorted(states.items())))
        if self.signature not in (None, signature) and not self.working:
            self.clear_steps()  # Lo que contaba ya no es cierto: el equipo ha cambiado.
        self.signature = signature
        if self.clients is None or [c.key for c in clients] != [c.key for c in self.clients]:
            self.build_rows(clients, states)
        self.clients = clients
        for client in clients:
            text, color = STATES[states[client.key]]
            self.badges[client.key].configure(text=text, foreground=color)
            note = client.hint
            if client.key.startswith("claude-desktop") and snap["desktop_open"] and states[client.key] != "ok":
                note = "ciérralo para conectar"
            self.notes[client.key].configure(text="·  " + note if note else "")

        relevant = [c.key for c in clients if c.selected or states[c.key] != "missing"]
        complete = bool(relevant) and all(states[key] == "ok" for key in relevant)
        connected = sum(states[key] == "ok" for key in relevant)
        stale = any(state == "stale" for state in states.values())
        if snap["broken"]:
            self.banner.configure(text="Instalación incompleta", foreground=PINK)
        elif stale:
            self.banner.configure(text="Actualización pendiente", foreground=PINK)
        elif complete:
            self.banner.configure(text="✓ Todo listo", foreground=INK)
        elif connected:
            self.banner.configure(text=f"{connected} de {len(relevant)} apps conectadas", foreground=INK)
        else:
            self.banner.configure(text="Sin instalar", foreground=MUTED)
        if not self.working:
            self.connect.configure(text="Reparar" if snap["broken"] else "Actualizar" if stale
                                   else "Reinstalar" if complete else "Instalar")
        self.can_remove = snap["installed"] or any(state != "missing" for state in states.values())
        self.validate()

    def validate(self) -> None:
        """Avisa mientras se escribe. El botón siempre responde: si falta algo, dice qué."""
        if self.snap is None:
            return
        self.key_label.configure(foreground=MUTED)
        saved = self.snap["account"].get("url")
        key = self.token.get().strip()
        try:
            url = installer.normalize_campus(self.url.get())
            same = bool(saved) and installer.normalize_campus(saved) == url
        except installer.InstallError:
            same = False
        if key and not (key.isascii() and key.isalnum()):
            hint = "solo letras y números"
            self.key_label.configure(foreground=PINK)
        elif key:
            hint = "no es tu contraseña"
        elif same:
            hint = "déjalo vacío para usar el guardado"
        else:
            hint = "no es tu contraseña"
        self.key_label.configure(text="TOKEN DE MOODLE  ·  " + hint)
        if not self.working:
            self.connect.configure(state="normal", background=INK)
            self.remove.configure(state="normal" if self.can_remove else "disabled")
            self.paint_remove()

    def paint_remove(self) -> None:
        self.outline.configure(background=OFF if str(self.remove["state"]) == "disabled" else INK)

    def clear_steps(self) -> None:
        self.steps = []
        self.log.grid_remove()

    def keep_on_screen(self) -> None:
        """Al crecer con los resultados, la ventana no debe quedar bajo la barra de tareas."""
        self.root.update_idletasks()
        limit = self.root.winfo_screenheight() - TASKBAR
        height = self.root.winfo_reqheight() + TITLE_BAR
        if self.root.winfo_y() + height > limit:
            self.root.geometry(f"+{self.root.winfo_x()}+{max(0, limit - height)}")

    def render_steps(self) -> None:
        self.log.grid()
        self.log.configure(state="normal")
        self.log.delete("1.0", "end")
        for index, (state, text) in enumerate(self.steps):
            mark, tag = MARKS[state]
            self.log.insert("end", ("\n" if index else "") + mark + text, tag)
        self.log.see("end")
        self.log.configure(state="disabled")
        self.keep_on_screen()

    def step(self, text: str, state: str = "run") -> None:
        """Da por terminado el paso en curso y añade el siguiente."""
        for current in self.steps:
            if current[0] == "run":
                current[0] = "ok"
        self.steps.append([state, text])
        self.render_steps()

    def fail(self, text: str) -> None:
        for current in self.steps:
            if current[0] == "run":
                current[0] = "err"
        self.steps.append(["err", text])
        self.render_steps()

    def toggle_token_help(self) -> None:
        if self.token_help.winfo_manager():
            self.token_help.grid_remove()
            self.token_help_toggle.configure(text="▸ Cómo obtener el token")
        else:
            self.token_help.grid()
            self.token_help_toggle.configure(text="▾ Cómo obtener el token")
        self.log.configure(height=3 if self.token_help.winfo_manager() else 6)
        self.keep_on_screen()

    def busy(self, working: bool, label: str = "") -> None:
        self.working = working
        if working:
            self.connect.configure(state="disabled", background=OFF, text=label)
            self.remove.configure(state="disabled")
            self.paint_remove()
            self.progress.grid()
            self.progress.start(12)
        else:
            self.progress.stop()
            self.progress.grid_remove()

    def report(self, text: str, key: str | None = None) -> None:
        self.events.put(("step", text))
        if key is not None:
            self.events.put(("badge", key))

    def ask(self, title: str, message: str) -> bool:
        """Pregunta desde el hilo de trabajo: los diálogos tienen que abrirse en el de la ventana."""
        answer: dict[str, bool] = {}
        ready = threading.Event()
        self.events.put(("ask", (title, message, answer, ready)))
        ready.wait()
        return answer["yes"]

    def clear_way(self, desktop: bool, servers: bool) -> None:
        """Pide permiso y cierra lo que tenga archivos o configuración bloqueados."""
        if not desktop and not servers:
            return
        reasons = []
        if desktop:
            reasons.append("Claude Desktop puede sobrescribir la configuración si sigue abierto.")
        if servers:
            reasons.append("Una app está usando Moodle AI Connect. Perderá esa conexión hasta que la reinicies.")
        if not self.ask("Hay aplicaciones abiertas", "\n\n".join(reasons)
                        + "\n\nGuarda tu trabajo. ¿Continuar?"):
            raise installer.InstallError("Cancelado. No se ha cambiado nada.")
        if desktop:
            self.report("Cerrando Claude Desktop")
            if not installer.close_claude_desktop():
                raise installer.InstallError("No se pudo cerrar Claude Desktop. Ciérralo a mano y "
                                             "vuelve a intentarlo. No se ha cambiado nada.")
        self.report("Deteniendo las conexiones abiertas")
        if not installer.stop_servers():
            raise installer.InstallError("No se pudo detener el servidor. Cierra las aplicaciones de IA y "
                                         "vuelve a intentarlo. No se ha cambiado nada.")

    def run_in_background(self, work, label: str) -> None:
        self.steps = []
        self.busy(True, label)

        def target() -> None:
            try:
                self.events.put(("done", work()))
            except installer.InstallError as error:
                self.events.put(("error", str(error)))
            except Exception:
                installer.log_error()
                self.events.put(("error", "No se pudo completar. Detalles en "
                                 + str(installer.log_path())))

        threading.Thread(target=target, daemon=True).start()

    def start_install(self) -> None:
        selected = [client for client in self.clients or [] if self.rows[client.key].get()]
        self.steps = []
        if not selected:
            self.fail("Marca al menos una aplicación.")
            return
        try:
            url, token = installer.resolve_account(self.url.get(), self.token.get())
        except installer.InstallError as error:
            self.fail(str(error))
            self.key_entry.focus_set()
            return
        self.mode = "install"

        def work():
            # La clave se prueba antes de tocar nada: si falla, el equipo queda como estaba.
            self.report("Probando conexión")
            info = installer.check_connection(url, token)
            self.events.put(("note", f"Cuenta: {info.get('user')} en {info.get('site')}"))
            missing = info.get("missing_functions") or []
            if missing:
                self.events.put(("note", "Consultas sin permiso: " + ", ".join(missing)))
            self.clear_way(*installer.needs_closing_for_install(selected))
            notes = installer.install(url, token, selected, self.report)
            self.report("Comprobando instalación")
            installer.verify_installed()
            return notes

        self.run_in_background(work, "Instalando…")

    def start_uninstall(self) -> None:
        if not messagebox.askyesno("Desinstalar", "¿Desinstalar Moodle AI Connect y borrar el token guardado?"):
            return
        self.mode = "uninstall"

        def work():
            self.clear_way(*installer.needs_closing_for_uninstall())
            return installer.uninstall(self.report)

        self.run_in_background(work, "Desinstalando…")

    def handle(self, kind: str, payload) -> None:
        if kind == "snapshot":
            if not self.working:
                self.apply(payload)
        elif kind == "step":
            self.step(payload)
        elif kind == "note":
            self.step(payload, "note")
        elif kind == "badge":
            if payload in self.badges:
                self.badges[payload].configure(text="…", foreground=PINK)
        elif kind == "ask":
            title, message, answer, ready = payload
            answer["yes"] = messagebox.askyesno(title, message, parent=self.root)
            ready.set()
        else:
            self.finish(kind, payload)

    def finish(self, kind: str, payload) -> None:
        if kind == "error":
            self.fail(payload)
        elif self.mode == "uninstall":
            self.step("Desinstalado" + (" de " + ", ".join(payload) if payload else ""), "done")
            self.step("Se conservan las copias de seguridad de tu configuración.", "note")
        else:
            self.token.set("")
            self.step("Listo. Ya puedes consultar Moodle desde tu IA.", "done")
            for note in payload:
                self.step(note, "note")
        self.busy(False)
        self.clients = None  # Vuelve a calcular qué casillas marcar según el nuevo estado.
        self.signature = None  # El resultado de esta operación se queda a la vista.
        self.refresh()

    def poll(self) -> None:
        try:
            while True:
                kind, payload = self.events.get_nowait()
                self.handle(kind, payload)
        except queue.Empty:
            pass
        self.root.after(100, self.poll)


def main() -> None:
    try:  # Texto nítido en pantallas con escala superior al 100 %.
        import ctypes
        ctypes.windll.shcore.SetProcessDpiAwareness(1)
    except (AttributeError, OSError):
        pass
    root = tk.Tk()
    window = SetupWindow(root)
    window.start_monitor()
    root.mainloop()


if __name__ == "__main__":
    main()
