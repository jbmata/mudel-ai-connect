"""Escribe dist/SHA256SUMS.txt con la huella de los archivos que se adjuntan a una release."""

import hashlib
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "local"))

from mudel_ai_connect_installer import VERSION  # noqa: E402

FILES = ("Mudel-AI-Connect-Setup.exe", f"mudel-ai-connect-plugin-{VERSION}.zip")


def write_checksums(dist: Path = ROOT / "dist") -> Path:
    lines = []
    for name in FILES:
        path = dist / name
        if not path.is_file():
            raise SystemExit(f"Falta {path}. Genera antes los paquetes.")
        lines.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {name}")
    output = dist / "SHA256SUMS.txt"
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return output


if __name__ == "__main__":
    output = write_checksums()
    print(output.read_text(encoding="utf-8"), end="")
