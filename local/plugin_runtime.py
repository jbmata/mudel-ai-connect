"""Instalación local y arranque del plugin sin guardar estado en su carpeta."""

from __future__ import annotations

import argparse
import base64
import getpass
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import venv
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent
PERMISSIONS = {
    "courses": "Asignaturas",
    "materials": "Materiales y archivos",
    "assignments": "Tareas",
    "calendar": "Calendario",
}


def data_dir() -> Path:
    if sys.platform == "win32":
        parent = Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local"))
    else:
        parent = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
    return parent / "mudel-ai-connect"


def credentials_path() -> Path:
    return data_dir() / "credentials.json"


def protected_credentials_path() -> Path:
    return data_dir() / "credentials.dpapi.json"


def _validate_permissions(values: dict[str, bool]) -> dict[str, bool]:
    if (not isinstance(values, dict) or set(values) != set(PERMISSIONS)
            or any(type(value) is not bool for value in values.values())):
        raise ValueError("Los permisos deben indicar sí o no para cada tipo de consulta.")
    return values


def read_permissions() -> dict[str, bool]:
    try:
        return _validate_permissions(json.loads((data_dir() / "permissions.json").read_text(encoding="utf-8")))
    except FileNotFoundError:
        return dict.fromkeys(PERMISSIONS, True)
    except (OSError, ValueError):
        raise ValueError("No se pudieron leer los permisos. Vuelve a guardarlos desde el instalador.") from None


def save_permissions(values: dict[str, bool]) -> None:
    _write_json(data_dir() / "permissions.json", _validate_permissions(values))


def _dpapi(data: bytes, protect: bool) -> bytes:
    """Cifra o descifra con la cuenta de Windows del usuario (DPAPI), sin dependencias."""
    import ctypes
    from ctypes import wintypes

    class Blob(ctypes.Structure):
        _fields_ = [("cbData", wintypes.DWORD), ("pbData", ctypes.POINTER(ctypes.c_char))]

    crypt32, kernel32 = ctypes.windll.crypt32, ctypes.windll.kernel32
    function = crypt32.CryptProtectData if protect else crypt32.CryptUnprotectData
    function.argtypes = [ctypes.POINTER(Blob), wintypes.LPCWSTR if protect else ctypes.c_void_p,
                         ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, wintypes.DWORD,
                         ctypes.POINTER(Blob)]
    function.restype = wintypes.BOOL
    kernel32.LocalFree.argtypes = [ctypes.c_void_p]
    buffer = ctypes.create_string_buffer(data, len(data))
    source = Blob(len(data), ctypes.cast(buffer, ctypes.POINTER(ctypes.c_char)))
    result = Blob()
    if not function(ctypes.byref(source), "mudel-ai-connect" if protect else None, None, None, None, 0,
                    ctypes.byref(result)):
        raise OSError("DPAPI rechazó la operación.")
    try:
        return ctypes.string_at(result.pbData, result.cbData)
    finally:
        kernel32.LocalFree(result.pbData)


def read_protected_credentials(path: Path) -> dict[str, str]:
    """Lee una cuenta cifrada con DPAPI, también la de una instalación con el nombre anterior."""
    try:
        values = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(values, dict) or any(not isinstance(values.get(key), str)
                                              for key in ("MOODLE_URL", "MOODLE_TOKEN_DPAPI")):
            raise ValueError
        token = _dpapi(base64.b64decode(values["MOODLE_TOKEN_DPAPI"], validate=True), False).decode("ascii")
        return {"MOODLE_URL": values["MOODLE_URL"], "MOODLE_TOKEN": token}
    except (OSError, ValueError, KeyError, AttributeError, TypeError):
        raise ValueError("No se pudo leer la clave guardada. Vuelve a configurar la cuenta con el instalador.") from None


def read_credentials(*, protect: bool | None = None) -> dict[str, str]:
    protected = protected_credentials_path()
    if protect is not False and protected.exists():
        return read_protected_credentials(protected)
    if protect is True:
        return {}
    path = credentials_path()
    if not path.exists():
        return {}
    try:
        values = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(values, dict) or any(not isinstance(values.get(key), str)
                                             for key in ("MOODLE_URL", "MOODLE_TOKEN")):
            raise ValueError
        return {key: values[key] for key in ("MOODLE_URL", "MOODLE_TOKEN")}
    except (OSError, ValueError):
        raise ValueError("No se pudo leer la configuración local del plugin. Vuelve a ejecutar setup.") from None


def save_credentials(url: str, token: str, *, protect: bool = False) -> None:
    """Guarda la cuenta. Con protect=True (Windows) la clave se cifra para el usuario actual."""
    url, token = url.strip().rstrip("/"), token.strip()
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or not parsed.hostname or parsed.username
            or parsed.password or parsed.query or parsed.fragment):
        raise ValueError("La dirección del campus debe ser HTTPS, sin credenciales ni parámetros.")
    if not token or not token.isascii() or not token.isalnum():
        raise ValueError("La clave de servicios web debe contener solo letras y números.")
    if protect:
        target = protected_credentials_path()
        content = {"MOODLE_URL": url,
                   "MOODLE_TOKEN_DPAPI": base64.b64encode(_dpapi(token.encode("ascii"), True)).decode("ascii")}
    else:
        target = credentials_path()
        content = {"MOODLE_URL": url, "MOODLE_TOKEN": token}
    _write_json(target, content)


def _write_json(target: Path, content: dict) -> None:
    target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=target.parent,
                                         delete=False) as stream:
            temporary = Path(stream.name)
            json.dump(content, stream)
        temporary.chmod(0o600)
        os.replace(temporary, target)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def runtime_python() -> Path:
    requirements = (ROOT / "requirements.txt").read_bytes()
    fingerprint = hashlib.sha256(requirements).hexdigest()[:12]
    environment = data_dir() / "runtime" / f"py{sys.version_info.major}{sys.version_info.minor}-{fingerprint}"
    return environment / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")


def setup(source_env: Path | None, configure: bool) -> None:
    python = runtime_python()
    environment = python.parent.parent
    if not python.is_file():
        print("Preparando el entorno Python del plugin…", file=sys.stderr)
        venv.EnvBuilder(with_pip=True).create(environment)
    subprocess.run([str(python), "-m", "pip", "install", "--disable-pip-version-check",
                    "-r", str(ROOT / "requirements.txt")], stdout=sys.stderr, check=True)
    (environment / ".mudel-ai-connect-ready").touch()
    if source_env is not None:
        # Capturar en memoria: la clave nunca se imprime en el terminal de instalación.
        result = subprocess.run([
            str(python), "-c",
            "import json,sys; from dotenv import dotenv_values; "
            "v=dotenv_values(sys.argv[1], interpolate=False); "
            "print(json.dumps({k:v.get(k,'') for k in ('MOODLE_URL','MOODLE_TOKEN')}))",
            str(source_env.resolve()),
        ], capture_output=True, text=True, check=True)
        values = json.loads(result.stdout)
        save_credentials(values["MOODLE_URL"], values["MOODLE_TOKEN"])
        print("Clave importada a la configuración local del usuario.")
    elif configure:
        url = input("URL de Moodle [https://sifo.comillas.edu]: ").strip() or "https://sifo.comillas.edu"
        token = getpass.getpass("Clave de servicios web (no se mostrará): ")
        save_credentials(url, token)
        print("Configuración guardada en el perfil local del usuario.")
    print("Entorno del plugin preparado.")


def run(check: bool = False) -> int:
    python = runtime_python()
    if not python.is_file() or not (python.parent.parent / ".mudel-ai-connect-ready").is_file():
        raise ValueError("Falta preparar el plugin. Ejecuta: python plugin_runtime.py setup")
    os.environ.setdefault("PYTHONIOENCODING", "utf-8")
    # En el paquete el servidor está junto al lanzador; en el repositorio, en ../shared.
    script = ROOT / "moodle_mcp.py"
    args = [str(python), str(script if script.is_file() else ROOT.parent / "shared" / "moodle_mcp.py")]
    if check:
        args.append("--check")
    # Esperar al hijo evita que el lanzador termine antes que el MCP en Windows.
    # El servidor lee la cuenta guardada importando este módulo, esté o no a su lado.
    paths = os.pathsep.join(filter(None, [str(ROOT), os.environ.get("PYTHONPATH")]))
    environment = {"MOODLE_URL": "", "MOODLE_TOKEN": "", **read_credentials(protect=False),
                   **os.environ, "PYTHONPATH": paths}
    return subprocess.call(args, env=environment)


def main() -> None:
    parser = argparse.ArgumentParser(description="Preparar o ejecutar Mudel AI Connect.")
    commands = parser.add_subparsers(dest="command", required=True)
    install = commands.add_parser("setup", help="Instalar dependencias y configurar la cuenta local.")
    install.add_argument("--from-env", type=Path, help="Importar MOODLE_URL y MOODLE_TOKEN de un archivo local.")
    install.add_argument("--no-config", action="store_true", help="Preparar solo el entorno, conservando las credenciales.")
    start = commands.add_parser("run", help="Arrancar el MCP por stdio.")
    start.add_argument("--check", action="store_true", help="Comprobar la conexión en vez de arrancar stdio.")
    args = parser.parse_args()
    if sys.version_info < (3, 11):
        parser.error("Se necesita Python 3.11 o superior.")
    try:
        if args.command == "setup":
            setup(args.from_env, not args.no_config)
        else:
            sys.exit(run(args.check))
    except ValueError as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
    except (OSError, subprocess.CalledProcessError):
        print("No se pudo preparar o ejecutar el plugin. Comprueba Python 3.11+, la instalación de dependencias y la configuración de Moodle.", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
