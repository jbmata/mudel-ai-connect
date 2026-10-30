"""Empaqueta exclusivamente los archivos públicos del plugin Moodle."""

import json
import shutil
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

ROOT = Path(__file__).resolve().parents[1]
FILES = (
    "plugin.json", "mcp.json", ".claude-plugin/plugin.json", ".mcp.json",
    "moodle_mcp.py", "plugin_runtime.py", "requirements.txt", ".env.example",
    "skills/moodle-study/SKILL.md", "skills/setup/SKILL.md",
    "assets/logo.png",
)


def build(root: Path = ROOT) -> Path:
    output = root / "dist" / "moodle-plugin-0.1.0.zip"
    paths = {name: root / name for name in FILES}
    paths["README.md"] = root / "PLUGIN.md"
    for path in paths.values():
        if not path.is_file():
            raise ValueError(f"Falta un archivo público del plugin: {path.name}")
    output.parent.mkdir(exist_ok=True)
    with ZipFile(output, "w", compression=ZIP_DEFLATED) as archive:
        for name, path in paths.items():
            archive.write(path, name)
    marketplace = root / "dist" / "moodle-marketplace"
    catalog_path = marketplace / ".agents" / "plugins" / "marketplace.json"
    plugin_path = marketplace / "plugins" / "moodle"
    allowed = {Path("plugins/moodle") / name for name in paths}
    allowed.add(Path(".agents/plugins/marketplace.json"))
    if marketplace.exists():
        for path in marketplace.rglob("*"):
            if path.is_symlink() or (path.is_file() and path.relative_to(marketplace) not in allowed):
                raise ValueError("El marketplace generado contiene archivos ajenos al paquete. No se ha copiado su contenido.")
    for name, source in paths.items():
        target = plugin_path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    catalog_path.parent.mkdir(parents=True, exist_ok=True)
    catalog_path.write_text(json.dumps({
        "name": "moodle-local",
        "plugins": [{"name": "moodle", "source": {"source": "local", "path": "./plugins/moodle"},
                     "policy": {"installation": "AVAILABLE", "authentication": "ON_INSTALL"},
                     "category": "Productivity"}],
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return output


if __name__ == "__main__":
    print(build())
