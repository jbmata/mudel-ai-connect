"""Compila dist/Mudel-AI-Connect-Setup.exe con el servidor MCP y Python dentro."""

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOCAL, SHARED = ROOT / "local", ROOT / "shared"
WORK = ROOT / "tmp" / "exe"
SERVER = "mudel-ai-connect-server"


def pyinstaller(*args: str) -> None:
    subprocess.run([sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean",
                    "--specpath", str(WORK), "--paths", str(LOCAL), "--paths", str(SHARED), *args], check=True, cwd=ROOT)


def build(dist: Path = ROOT / "dist") -> Path:
    sys.path.insert(0, str(LOCAL))
    from mudel_ai_connect_installer import write_server_manifest

    shutil.rmtree(WORK, ignore_errors=True)
    separator = ";" if sys.platform == "win32" else ":"
    # El servidor va en carpeta (arranca al instante) y con consola, porque habla por stdio.
    pyinstaller("--onedir", "--console", "--name", SERVER, "--exclude-module", "tkinter",
                "--add-data", f"{ROOT / 'LICENSE'}{separator}licenses",
                "--distpath", str(WORK / "server"), "--workpath", str(WORK / "work-server"),
                str(SHARED / "moodle_mcp.py"))
    write_server_manifest(WORK / "server" / SERVER)
    # El instalador es un único archivo sin consola, con el servidor y el logo dentro.
    pyinstaller("--onefile", "--windowed", "--name", "Mudel-AI-Connect-Setup",
                "--icon", str(LOCAL / "assets" / "logo.ico"),
                "--add-data", f"{WORK / 'server' / SERVER}{separator}{SERVER}",
                "--add-data", f"{LOCAL / 'assets' / 'logo-setup.png'}{separator}assets",
                "--distpath", str(dist), "--workpath", str(WORK / "work-setup"),
                str(LOCAL / "mudel_ai_connect_setup.py"))
    return dist / "Mudel-AI-Connect-Setup.exe"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, default=ROOT / "dist",
                        help="Carpeta de salida; útil si dist/Mudel-AI-Connect-Setup.exe está abierto y bloqueado.")
    print(build(parser.parse_args().dist))
