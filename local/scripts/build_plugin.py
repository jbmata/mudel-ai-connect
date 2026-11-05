"""Empaqueta exclusivamente los archivos públicos de Mudel AI Connect."""

import argparse
import json
import shutil
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

ROOT = Path(__file__).resolve().parents[2]
# Ruta dentro del ZIP -> origen en el repositorio. El ZIP es plano; el repositorio, no.
SOURCES = {
    "LICENSE": "LICENSE",
    "plugin.json": "local/plugin.json",
    "mcp.json": "local/mcp.json",
    ".claude-plugin/plugin.json": "local/.claude-plugin/plugin.json",
    ".mcp.json": "local/.mcp.json",
    "moodle_mcp.py": "shared/moodle_mcp.py",
    "plugin_runtime.py": "local/plugin_runtime.py",
    "requirements.txt": "shared/requirements.txt",
    ".env.example": "local/.env.example",
    "skills/mudel-ai-connect-study/SKILL.md": "local/skills/mudel-ai-connect-study/SKILL.md",
    "skills/mudel-ai-connect-study/agents/openai.yaml": "local/skills/mudel-ai-connect-study/agents/openai.yaml",
    "skills/setup/SKILL.md": "local/skills/setup/SKILL.md",
    "skills/setup/agents/openai.yaml": "local/skills/setup/agents/openai.yaml",
    "assets/logo.png": "shared/assets/logo.png",
}
README = "local/PLUGIN.md"
FILES = tuple(SOURCES)


def build(root: Path = ROOT) -> Path:
    version = json.loads((root / SOURCES["plugin.json"]).read_text(encoding="utf-8"))["version"]
    output = root / "dist" / f"mudel-ai-connect-plugin-{version}.zip"
    paths = {name: root / source for name, source in SOURCES.items()}
    paths["README.md"] = root / README
    for path in paths.values():
        if not path.is_file():
            raise ValueError(f"Falta un archivo público del plugin: {path.name}")
    output.parent.mkdir(exist_ok=True)
    with ZipFile(output, "w", compression=ZIP_DEFLATED) as archive:
        for name, path in paths.items():
            archive.write(path, name)
    marketplace = root / "dist" / "mudel-ai-connect-marketplace"
    catalog_path = marketplace / ".agents" / "plugins" / "marketplace.json"
    plugin_path = marketplace / "plugins" / "mudel-ai-connect"
    allowed = {Path("plugins/mudel-ai-connect") / name for name in paths}
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
        "name": "mudel-ai-connect-local",
        "interface": {"displayName": "Mudel AI Connect"},
        "plugins": [{"name": "mudel-ai-connect", "source": {"source": "local", "path": "./plugins/mudel-ai-connect"},
                     "policy": {"installation": "AVAILABLE", "authentication": "ON_INSTALL"},
                     "category": "Productivity"}],
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    print(build())
