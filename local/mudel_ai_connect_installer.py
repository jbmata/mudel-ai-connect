"""Instala Mudel AI Connect en este equipo y lo registra en los clientes de IA."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import tomllib
import traceback
import urllib.request
from collections.abc import Callable
from contextlib import ExitStack
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import urlsplit

from plugin_runtime import read_protected_credentials
from plugin_runtime import (
    PERMISSIONS, credentials_path, data_dir, protected_credentials_path, read_credentials, read_permissions,
    save_credentials, save_permissions, default_download_folder, read_download_folder, save_download_folder,
)

SERVER_NAME = "mudel-ai-connect"
# Nombre anterior de la app («Moodle AI Connect»): sus instalaciones se migran al instalar.
LEGACY_NAME = "moodle-ai-connect"
VERSION = "0.2.0"  # Debe coincidir con local/plugin.json y la etiqueta de la release (vX.Y.Z).
RELEASES = "https://api.github.com/repos/jbmata/mudel-ai-connect/releases?per_page=20"
SERVER_DIR = "mudel-ai-connect-server"
SERVER_EXE = "mudel-ai-connect-server.exe"
MANIFEST = "mudel-ai-connect-install.json"
DEFAULT_CAMPUS = "https://sifo.comillas.edu"
ROOT = Path(__file__).resolve().parent


class InstallError(Exception):
    """Fallo que se puede explicar al usuario sin mostrar rutas ni claves."""


def home() -> Path:
    return Path(os.environ.get("USERPROFILE") or Path.home())


def bundled_server() -> Path | None:
    """Carpeta del servidor empaquetada dentro del instalador, si es el .exe."""
    base = getattr(sys, "_MEIPASS", None)
    folder = Path(base) / SERVER_DIR if base else None
    return folder if folder is not None and (folder / SERVER_EXE).is_file() else None


def installed_server() -> Path:
    return data_dir() / "bin" / SERVER_DIR


def server_command() -> list[str]:
    """Comando que lanzan los clientes: el .exe instalado, o el script en desarrollo."""
    if bundled_server() is not None or (installed_server() / SERVER_EXE).is_file():
        return [str(installed_server() / SERVER_EXE)]
    return [sys.executable, str(ROOT.parent / "shared" / "moodle_mcp.py")]


def check_command() -> list[str]:
    """Servidor para probar la conexión sin haber cambiado todavía nada en el equipo."""
    source = bundled_server()
    return [str(source / SERVER_EXE)] if source is not None else server_command()


def _digest(folder: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(item for item in folder.rglob("*") if item.is_file() and item.name != MANIFEST):
        digest.update(path.relative_to(folder).as_posix().encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def write_server_manifest(folder: Path) -> None:
    files = {path.relative_to(folder).as_posix(): path.stat().st_size
             for path in folder.rglob("*") if path.is_file() and path.name != MANIFEST}
    _write_atomic(folder / MANIFEST, json.dumps({"digest": _digest(folder), "files": files}))


def _read_manifest(folder: Path | None = None) -> dict:
    try:
        manifest = json.loads(((folder or installed_server()) / MANIFEST).read_text(encoding="utf-8"))
        return manifest if isinstance(manifest, dict) else {}
    except (OSError, ValueError):
        return {}


def server_intact() -> bool:
    """Comprueba que no falte ningún archivo: un servidor medio borrado sigue teniendo su .exe."""
    files = _read_manifest().get("files")
    if not isinstance(files, dict) or not files:
        return False
    root = installed_server()
    try:
        return all((root / name).stat().st_size == size for name, size in files.items())
    except OSError:
        return False


def server_up_to_date() -> bool:
    source = bundled_server()
    return source is None or (server_intact() and _read_manifest().get("digest")
                              == (_read_manifest(source).get("digest") or _digest(source)))


def _retry(action: Callable[[], object], attempts: int = 6) -> None:
    """Tras cerrar un proceso, Windows o el antivirus aún pueden retener sus archivos unos segundos."""
    for attempt in range(attempts):
        try:
            action()
            return
        except OSError:
            if attempt == attempts - 1:
                raise
            time.sleep(1)


def _remove_readonly(action, path, error) -> None:
    if not isinstance(error[1], PermissionError):
        raise error[1]
    os.chmod(path, stat.S_IRWXU if os.path.isdir(path) else stat.S_IREAD | stat.S_IWRITE)
    action(path)


def _clean_leftovers() -> None:
    for path in installed_server().parent.glob(SERVER_DIR + ".*"):
        try:
            _retry(lambda: shutil.rmtree(path, onerror=_remove_readonly))
        except OSError:
            raise InstallError("No se pudieron borrar archivos de una instalación anterior. "
                               "Cierra las aplicaciones de IA, comprueba los permisos de la carpeta "
                               "y vuelve a desinstalar.") from None


def _set_aside(target: Path) -> Path:
    # Renombrar es atómico: con un archivo en uso falla sin haber borrado nada.
    old = target.with_name(f"{target.name}.old-{os.getpid()}-{time.time_ns()}")
    _retry(lambda: os.replace(target, old))
    return old


def install_server(rollback: ExitStack | None = None) -> list[str]:
    source = bundled_server()
    if source is None or server_up_to_date():
        return server_command()
    target = installed_server()
    staging = target.with_name(target.name + ".new")
    try:
        target.parent.mkdir(parents=True, exist_ok=True)
        _clean_leftovers()
        shutil.copytree(source, staging)
        write_server_manifest(staging)
        old = _set_aside(target) if target.exists() else None
        try:
            _retry(lambda: os.replace(staging, target))
        except OSError:
            if old is not None:
                os.replace(old, target)
            raise
    except OSError:
        shutil.rmtree(staging, ignore_errors=True)
        raise InstallError("No se pudo copiar el servidor porque alguna aplicación lo tiene abierto. "
                           "Cierra las aplicaciones de IA y vuelve a intentarlo. No se ha cambiado nada.") from None
    if rollback is not None:
        rollback.callback(_restore_server, old)
    else:
        _clean_leftovers()
    return server_command()


def _restore_server(previous: Path | None) -> None:
    target = installed_server()
    if target.exists():
        _set_aside(target)
    if previous is not None:
        _retry(lambda: os.replace(previous, target))
    _clean_leftovers()


def _remove_server() -> None:
    target = installed_server()
    if target.exists():
        try:
            _set_aside(target)
        except OSError:
            raise InstallError("No se pudo borrar el servidor porque alguna aplicación lo tiene abierto. "
                               "Cierra las aplicaciones de IA y vuelve a intentarlo. No se ha cambiado nada.") from None
    _clean_leftovers()


# --- Clientes ---------------------------------------------------------------

@dataclass(frozen=True)
class Client:
    key: str
    name: str
    path: Path
    kind: str  # "json" o "toml"
    note: str = ""
    selected: bool = True
    hint: str = ""


def claude_desktop_dirs() -> tuple[list[Path], list[Path]]:
    """Carpetas de datos de Claude Desktop: (las que lee ahora, las antiguas que ya no lee).

    Las versiones recientes (al menos la 4.16336, de noviembre de 2026) trasladan sus datos a
    %LOCALAPPDATA%\\Claude-Data\\claude_<id>. Las anteriores usaban %APPDATA%\\Claude o, la de la
    Microsoft Store, su carpeta de Packages. La antigua no se borra al migrar, así que su mera
    existencia no indica que se use: escribir ahí deja a Claude sin el servidor.
    """
    roaming = Path(os.environ.get("APPDATA") or home() / "AppData" / "Roaming")
    local = Path(os.environ.get("LOCALAPPDATA") or home() / "AppData" / "Local")
    current = sorted(path for path in (local / "Claude-Data").glob("claude*") if path.is_dir())
    legacy = [roaming / "Claude", *sorted((local / "Packages").glob("Claude_*/LocalCache/Roaming/Claude"))]
    legacy = [path for path in legacy if path.is_dir()]
    return (current, legacy) if current else (legacy, [])


def _forget_old_desktop_entries() -> None:
    """Quita nuestra entrada de las carpetas que Claude Desktop ya no lee, para no dejar restos."""
    for folder in claude_desktop_dirs()[1]:
        old = Client("claude-desktop-old", "Claude Desktop", folder / "claude_desktop_config.json", "json")
        try:
            unregister(old)
        except (InstallError, OSError, ValueError):
            pass


def detect_clients() -> list[Client]:
    found: list[Client] = []
    for index, folder in enumerate(claude_desktop_dirs()[0]):
        found.append(Client(f"claude-desktop-{index}", "Claude Desktop",
                            folder / "claude_desktop_config.json", "json",
                            "Abre Claude Desktop de nuevo para cargarlo."))
    if (home() / ".claude.json").is_file():
        found.append(Client("claude-code", "Claude Code", home() / ".claude.json", "json",
                            "Abre una sesión nueva de Claude Code."))
    if (home() / ".codex").is_dir():
        config = home() / ".codex" / "config.toml"
        text = config.read_text(encoding="utf-8", errors="replace") if config.is_file() else ""
        has_plugin = re.search(r"""^\[plugins\.["']?(?:moodle|moodle-ai-connect|mudel-ai-connect)@""",
                               text, re.MULTILINE) is not None
        found.append(Client("codex", "ChatGPT Work local / Codex", config, "toml",
                            "Abre una tarea Work local nueva en ChatGPT o una sesión nueva de Codex.",
                            selected=not has_plugin,
                            hint="ya tiene Mudel AI Connect; no hace falta" if has_plugin else ""))
    return found


CLAUDE_DESKTOP_PATHS = ("\\windowsapps\\claude_", "\\anthropicclaude\\", "\\programs\\claude\\")
HIDDEN = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def _process_paths() -> dict[int, str]:
    """Ruta del ejecutable de cada proceso. Usa la API de Windows: es rápido y no abre consolas."""
    if sys.platform != "win32":
        return {}
    import ctypes
    from ctypes import wintypes

    psapi, kernel32 = ctypes.WinDLL("psapi"), ctypes.WinDLL("kernel32")
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR,
                                                    ctypes.POINTER(wintypes.DWORD)]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    pids = (wintypes.DWORD * 8192)()
    used = wintypes.DWORD()
    if not psapi.EnumProcesses(pids, ctypes.sizeof(pids), ctypes.byref(used)):
        return {}
    found = {}
    for pid in pids[:used.value // ctypes.sizeof(wintypes.DWORD)]:
        handle = kernel32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
        if not handle:
            continue
        try:
            buffer = ctypes.create_unicode_buffer(1024)
            size = wintypes.DWORD(len(buffer))
            if kernel32.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
                found[pid] = buffer.value
        finally:
            kernel32.CloseHandle(handle)
    return found


def running_claude_desktop() -> list[int]:
    """Procesos de la aplicación Claude Desktop, distinguidos por su ruta de instalación."""
    return [pid for pid, path in _process_paths().items()
            if any(marker in path.lower() for marker in CLAUDE_DESKTOP_PATHS)]


def legacy_dir() -> Path:
    """Carpeta de datos de la app con su nombre anterior."""
    return data_dir().parent / LEGACY_NAME


def running_servers() -> list[int]:
    """Copias en marcha del servidor instalado por este programa (también con el nombre anterior)."""
    folder = os.path.normcase(str(installed_server()))
    old = os.path.normcase(str(legacy_dir() / "bin"))
    return [pid for pid, path in _process_paths().items()
            if os.path.normcase(path).startswith((folder + os.sep, folder + ".old-", old + os.sep))]


def _kill(find: Callable[[], list[int]]) -> bool:
    for pid in find():
        subprocess.run(["taskkill", "/F", "/PID", str(pid)], capture_output=True, creationflags=HIDDEN)
    for _ in range(10):
        if not find():
            return True
        time.sleep(1)
    return False


def close_claude_desktop() -> bool:
    """Cierra Claude Desktop. Devuelve True si ya no queda ningún proceso suyo."""
    return _kill(running_claude_desktop)


def stop_servers() -> bool:
    """Detiene las copias en marcha del servidor. La aplicación que las usa pierde esa conexión."""
    return _kill(running_servers)


def _write_atomic(path: Path, text: str | bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile("wb", dir=path.parent, delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(text.encode("utf-8") if isinstance(text, str) else text)
        temporary.chmod(0o600)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _backup(path: Path) -> None:
    backup = path.with_name(path.name + ".mudel-ai-connect-backup")
    if path.is_file() and not backup.exists():
        shutil.copyfile(path, backup)


def _restore_file(path: Path, previous: bytes | None) -> None:
    if previous is None:
        path.unlink(missing_ok=True)
    elif not path.is_file() or path.read_bytes() != previous:
        _write_atomic(path, previous)


def _load_json(path: Path) -> dict:
    try:
        if not path.is_file() or not path.read_text(encoding="utf-8").strip():
            return {}
        value = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(value, dict) or not isinstance(value.get("mcpServers", {}), dict):
            raise ValueError
        return value
    except (ValueError, OSError):
        raise InstallError(f"No se pudo leer {path.name}. No se ha modificado; revisa que sea "
                           "un JSON válido.") from None


def _toml_text(value: str) -> str:
    return json.dumps(value)  # una cadena JSON es una cadena TOML básica válida


def _toml_without_server(text: str, name: str = SERVER_NAME) -> str:
    kept, skipping = [], False
    root_key = r'''(?:mcp_servers|"mcp_servers"|'mcp_servers')'''
    server_key = rf'''(?:{name}|"{name}"|'{name}')'''
    for line in text.splitlines():
        header = re.match(r"\s*\[+\s*([^\]]+?)\s*\]+\s*(#.*)?$", line)
        if header:
            skipping = re.fullmatch(rf"{root_key}\s*\.\s*{server_key}(\s*\..*)?", header.group(1)) is not None
        if not skipping:
            kept.append(line)
    return "\n".join(kept).rstrip() + ("\n" if kept else "")


def _configuration_with_server(client: Client, command: list[str]) -> str:
    if client.kind == "json":
        config = _load_json(client.path)
        entry = {"command": command[0], "args": command[1:], "env": {}}
        if client.key == "claude-code":
            entry = {"type": "stdio", **entry}
        config.setdefault("mcpServers", {})[SERVER_NAME] = entry
        return json.dumps(config, ensure_ascii=False, indent=2) + "\n"
    text = client.path.read_text(encoding="utf-8") if client.path.is_file() else ""
    block = (f"[mcp_servers.{SERVER_NAME}]\ncommand = {_toml_text(command[0])}\n"
             f"args = [{', '.join(_toml_text(arg) for arg in command[1:])}]\n")
    result = (_toml_without_server(text) + ("\n" if text.strip() else "") + block)
    try:
        tomllib.loads(text)
        tomllib.loads(result)
    except tomllib.TOMLDecodeError:
        raise InstallError(f"No se pudo editar {client.path.name} sin riesgo. No se ha modificado.") from None
    return result


def register(client: Client, command: list[str]) -> None:
    result = _configuration_with_server(client, command)
    _backup(client.path)
    _write_atomic(client.path, result)


def unregister(client: Client, name: str = SERVER_NAME) -> bool:
    if not client.path.is_file():
        return False
    if client.kind == "json":
        config = _load_json(client.path)
        if name not in config.get("mcpServers", {}):
            return False
        del config["mcpServers"][name]
        _write_atomic(client.path, json.dumps(config, ensure_ascii=False, indent=2) + "\n")
        return True
    text = client.path.read_text(encoding="utf-8")
    try:
        # Con un TOML roto no se sabe qué líneas son nuestras: no se toca.
        if name not in tomllib.loads(text).get("mcp_servers", {}):
            return False
        result = _toml_without_server(text, name)
        if name in tomllib.loads(result).get("mcp_servers", {}):
            raise ValueError
    except ValueError:
        raise InstallError(f"No se pudo editar {client.path.name} sin riesgo. No se ha modificado.") from None
    _write_atomic(client.path, result)
    return True


# --- Cuenta -----------------------------------------------------------------

def normalize_campus(url: str) -> str:
    url = url.strip().rstrip("/")
    if url and "://" not in url:
        url = "https://" + url
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or not parsed.hostname or parsed.username
            or parsed.password or parsed.query or parsed.fragment):
        raise InstallError("La dirección del campus debe ser HTTPS, por ejemplo https://sifo.comillas.edu.")
    return url


def _run_check(command: list[str], environment: dict[str, str]) -> dict:
    try:
        result = subprocess.run([*command, "--check"], env=environment, capture_output=True,
                                text=True, encoding="utf-8", errors="replace", timeout=90,
                                creationflags=HIDDEN)
    except (OSError, subprocess.TimeoutExpired):
        raise InstallError("No se pudo ejecutar la comprobación de conexión.") from None
    if result.returncode != 0:
        message = (result.stderr or "").strip().splitlines()
        raise InstallError(message[-1][:300] if message else "Moodle no aceptó la conexión.")
    try:
        return json.loads(result.stdout)
    except ValueError:
        raise InstallError("La comprobación no devolvió un resultado válido.") from None


def check_connection(url: str, token: str, command: list[str] | None = None) -> dict:
    """Ejecuta el propio servidor con --check, para probar el mismo camino que usará la IA."""
    url = normalize_campus(url)
    if not token.strip().isalnum() or not token.strip().isascii():
        raise InstallError("El token debe contener solo letras y números. "
                           "No es tu contraseña.")
    return _run_check(command or check_command(), {
        **os.environ, "MOODLE_URL": url, "MOODLE_TOKEN": token.strip(), "PYTHONIOENCODING": "utf-8"})


def verify_installed() -> dict:
    """Prueba lo que quedó en el disco: el servidor instalado con la clave guardada."""
    environment = {key: value for key, value in os.environ.items()
                   if key not in ("MOODLE_URL", "MOODLE_TOKEN")}
    return _run_check(server_command(), {**environment, "PYTHONIOENCODING": "utf-8"})


def registered_command(client: Client, name: str = SERVER_NAME) -> str | None:
    """Comando con el que el cliente tiene registrado el servidor, o None si no lo tiene."""
    try:
        if client.kind == "json":
            entry = _load_json(client.path).get("mcpServers", {}).get(name)
        else:
            text = client.path.read_text(encoding="utf-8") if client.path.is_file() else ""
            entry = tomllib.loads(text).get("mcp_servers", {}).get(name)
    except (InstallError, OSError, tomllib.TOMLDecodeError):
        return None
    command = entry.get("command") if isinstance(entry, dict) else None
    return command if isinstance(command, str) else None


def is_registered(client: Client, name: str = SERVER_NAME) -> bool:
    return registered_command(client, name) is not None


def client_state(client: Client) -> str:
    """«ok»; «stale» si apunta a otro servidor o este está roto; «missing» si no está registrado."""
    command = registered_command(client)
    if command is None:
        return "missing"
    current = Path(server_command()[0])
    same = os.path.normcase(os.path.normpath(command)) == os.path.normcase(os.path.normpath(current))
    if not same or not current.is_file():
        return "stale"
    ours = os.path.normcase(str(current.parent)) == os.path.normcase(str(installed_server()))
    return "stale" if ours and (not server_intact() or not server_up_to_date()) else "ok"


def has_install() -> bool:
    """¿Queda algo de este programa en el equipo, aunque no esté registrado en ningún cliente?"""
    return (installed_server().exists() or protected_credentials_path().exists()
            or any(installed_server().parent.glob(SERVER_DIR + ".*")))


def saved_account() -> dict[str, str]:
    """Campus de la cuenta guardada por este programa. No descifra ni devuelve la clave.

    El credentials.json del plugin por Python no cuenta: es de otra instalación y
    «Desinstalar» no lo borra.
    """
    for path in _saved_credentials():
        try:
            url = json.loads(path.read_text(encoding="utf-8"))["MOODLE_URL"]
        except (OSError, ValueError, KeyError, TypeError):
            return {}
        return {"url": url} if isinstance(url, str) and url else {}
    return {}


def _saved_credentials() -> list[Path]:
    """El token cifrado de esta app o, si aún no hay, el de la instalación con el nombre anterior."""
    for path in (protected_credentials_path(), legacy_dir() / "credentials.dpapi.json"):
        if path.is_file():
            return [path]
    return []


def _forget_legacy() -> list[str]:
    """Retira lo que dejó la app con su nombre anterior. Devuelve lo que no se pudo quitar.

    No borra el entorno ni el credentials.json del plugin por Python: alguien podría seguir usándolo.
    """
    problems = []
    clients = detect_clients() + [
        Client("claude-desktop-old", "Claude Desktop", folder / "claude_desktop_config.json", "json")
        for folder in claude_desktop_dirs()[1]]
    for client in clients:
        try:
            unregister(client, LEGACY_NAME)
        except (InstallError, OSError, ValueError):
            problems.append(f"No se pudo quitar la entrada antigua «{LEGACY_NAME}» de {client.name}.")
    old = legacy_dir()
    if (old / "bin").exists():
        try:
            aside = old / f"bin.old-{os.getpid()}-{time.time_ns()}"
            os.replace(old / "bin", aside)  # atómico: si algo lo usa, falla sin borrar nada
            shutil.rmtree(aside, ignore_errors=True)
        except OSError:
            problems.append("No se pudo borrar el servidor de la versión anterior: alguna aplicación lo usa.")
    for name in ("credentials.dpapi.json", "setup.log"):
        try:
            (old / name).unlink(missing_ok=True)
        except OSError:
            problems.append(f"No se pudo borrar {name} de la versión anterior.")
    try:
        old.rmdir()  # solo si ha quedado vacía
    except OSError:
        pass
    return problems


def snapshot() -> dict:
    """Foto de lo que hay ahora mismo en el equipo, leída siempre del disco y de los procesos."""
    clients = detect_clients()
    return {
        "clients": clients,
        "states": {client.key: client_state(client) for client in clients},
        "installed": has_install(),
        "broken": installed_server().exists() and not server_intact(),
        "account": saved_account(),
        "desktop_open": bool(running_claude_desktop()),
    }


def _version_tuple(text: str) -> tuple[int, ...]:
    return tuple(int(number) for number in re.findall(r"\d+", text)[:3])


def newer_release(timeout: float = 5) -> dict[str, str] | None:
    """La release publicada más reciente si es más nueva que esta copia. Sin red o sin releases: None.

    Usa la lista y no /releases/latest porque esa omite las betas marcadas como pre-release.
    """
    request = urllib.request.Request(RELEASES, headers={
        "Accept": "application/vnd.github+json", "User-Agent": f"mudel-ai-connect/{VERSION}"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            releases = json.loads(response.read(1_000_000))
    except (OSError, ValueError):
        return None
    best = None
    for release in releases if isinstance(releases, list) else []:
        tag, url = release.get("tag_name"), release.get("html_url") if isinstance(release, dict) else None
        if (isinstance(release, dict) and not release.get("draft") and isinstance(tag, str)
                and isinstance(url, str) and url.startswith("https://github.com/") and _version_tuple(tag)):
            if best is None or _version_tuple(tag) > _version_tuple(best["version"]):
                best = {"version": tag, "url": url}
    return best if best and _version_tuple(best["version"]) > _version_tuple(VERSION) else None


def diagnostics() -> str:
    """Resumen para pedir ayuda: versiones, estados y rutas. Nunca incluye el token."""
    snap = snapshot()
    current, legacy = claude_desktop_dirs()
    account = snap["account"].get("url")
    lines = [
        f"Mudel AI Connect {VERSION} ({'instalador' if getattr(sys, 'frozen', False) else 'desarrollo'})",
        f"Windows {platform.version()}",
        f"Servidor: {'instalado' if installed_server().exists() else 'sin instalar'}, "
        f"{'completo' if server_intact() else 'incompleto'}, copias en marcha: {len(running_servers())}",
        f"Token guardado: {'sí, campus ' + account if account else 'no'}",
        f"Claude Desktop abierto: {'sí' if snap['desktop_open'] else 'no'}",
        "Carpeta de Claude Desktop: " + (", ".join(map(str, current)) or "no encontrada"),
        "Carpetas antiguas de Claude Desktop: " + (", ".join(map(str, legacy)) or "ninguna"),
        f"Instalación con el nombre anterior: {'sí' if legacy_dir().exists() else 'no'}",
        "Aplicaciones:",
        *[f"  {client.name}: {snap['states'][client.key]} ({client.path})" for client in snap["clients"]],
    ]
    if log_path().is_file():
        tail = log_path().read_text(encoding="utf-8", errors="replace").splitlines()[-40:]
        lines += ["", "Últimas líneas de setup.log:", *tail]
    return "\n".join(lines)


def log_path() -> Path:
    return data_dir() / "setup.log"


def log_error() -> None:
    """El .exe no tiene consola: sin esto, un error inesperado no dejaría rastro."""
    try:
        log_path().parent.mkdir(parents=True, exist_ok=True)
        with log_path().open("a", encoding="utf-8") as stream:
            stream.write(f"--- {time.strftime('%Y-%m-%d %H:%M:%S')} Mudel AI Connect {VERSION}\n"
                         f"{traceback.format_exc()}\n")
    except OSError:
        pass


def resolve_account(url: str, token: str) -> tuple[str, str]:
    """Con la clave vacía reutiliza la guardada, pero solo para el mismo campus."""
    url = normalize_campus(url)
    if token.strip():
        return url, token.strip()
    try:
        saved = read_protected_credentials(_saved_credentials()[0]) if _saved_credentials() else {}
        if saved.get("MOODLE_TOKEN") and normalize_campus(saved.get("MOODLE_URL", "")) == url:
            return url, saved["MOODLE_TOKEN"]
    except (ValueError, InstallError):
        pass
    raise InstallError("Introduce tu token de Moodle.")


def needs_closing_for_install(clients: list[Client]) -> tuple[bool, bool]:
    """Qué hay que cerrar antes de instalar: (Claude Desktop, copias del servidor en uso)."""
    desktop = any(client.key.startswith("claude-desktop") for client in clients)
    return (desktop and bool(running_claude_desktop()),
            not server_up_to_date() and bool(running_servers()))


def needs_closing_for_uninstall() -> tuple[bool, bool]:
    desktop = any(client.key.startswith("claude-desktop")
                  and (is_registered(client) or is_registered(client, LEGACY_NAME))
                  for client in detect_clients())
    return desktop and bool(running_claude_desktop()), bool(running_servers())


def _ignore(_text: str, _client_key: str | None = None) -> None:
    pass


def install(url: str, token: str, clients: list[Client],
            progress: Callable[..., None] = _ignore) -> list[str]:
    """Guarda la cuenta, instala el servidor y lo registra. Devuelve avisos para el usuario."""
    url = normalize_campus(url)
    if any(client.key.startswith("claude-desktop") for client in clients) and running_claude_desktop():
        # Abierto, Claude Desktop reescribe su configuración y borra lo que añadimos.
        raise InstallError("Claude Desktop está abierto. Ciérralo del todo (también desde la "
                           "bandeja del sistema) y vuelve a intentarlo.")
    for client in clients:
        _configuration_with_server(client, server_command())
    account_path = protected_credentials_path() if sys.platform == "win32" else credentials_path()
    paths = [account_path] + [client.path for client in clients]
    previous = {path: path.read_bytes() if path.exists() else None for path in paths}
    notes = []
    with ExitStack() as rollback:
        progress("Instalando el servidor")
        command = install_server(rollback)
        for path, content in previous.items():
            rollback.callback(_restore_file, path, content)
        progress("Guardando el token cifrado")
        save_credentials(url, token, protect=sys.platform == "win32")
        for client in clients:
            progress(f"Conectando {client.name}", client.key)
            register(client, command)
            if not is_registered(client):
                raise InstallError(f"No se pudo comprobar el registro en {client.name}.")
            notes.append(f"{client.name}: {client.note}" if client.note else client.name)
        progress("Comprobando instalación")
        verify_installed()
        rollback.pop_all()
    _forget_old_desktop_entries()  # Solo tras una instalación comprobada.
    notes += _forget_legacy()
    try:
        _clean_leftovers()
    except InstallError as error:
        notes.append(str(error))
    return notes


def uninstall(progress: Callable[..., None] = _ignore) -> list[str]:
    """Quita el servidor, las entradas y la clave. Si algo queda, lo dice en vez de darlo por hecho."""
    desktop, servers = needs_closing_for_uninstall()
    if desktop:
        raise InstallError("Claude Desktop está abierto y volvería a escribir su configuración. "
                           "Ciérralo del todo y vuelve a intentarlo. No se ha cambiado nada.")
    if servers:
        raise InstallError("Alguna aplicación está usando Mudel AI Connect. Ciérrala y vuelve a "
                           "intentarlo. No se ha cambiado nada.")
    # Primero el servidor: es lo único que puede fallar por estar en uso.
    progress("Borrando el servidor")
    _remove_server()
    removed, failed = [], []
    for client in detect_clients():
        progress(f"Desconectando {client.name}", client.key)
        try:
            if unregister(client):
                removed.append(client.name)
            if is_registered(client):
                failed.append(client.name)
        except (InstallError, OSError, ValueError):
            failed.append(client.name)
    _forget_old_desktop_entries()
    problems = _forget_legacy()
    progress("Borrando el token")
    protected_credentials_path().unlink(missing_ok=True)
    if failed:
        raise InstallError("Se han borrado el servidor y el token, pero no se pudo quitar la entrada de "
                           + ", ".join(failed) + ". Revisa su archivo de configuración.")
    if problems:
        raise InstallError(" ".join(problems))
    log_path().unlink(missing_ok=True)
    return removed
