"""Empaqueta exclusivamente los archivos públicos del plugin Moodle."""

import argparse
import json
import shutil
from pathlib import Path
from urllib.parse import urlsplit
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


def build_remote(url: str, root: Path = ROOT) -> Path:
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or not parsed.hostname or parsed.username is not None
            or parsed.password is not None or parsed.query or parsed.fragment or parsed.path != "/mcp"):
        raise ValueError("El conector remoto debe ser una URL HTTPS /mcp sin credenciales ni parámetros.")
    manifest = json.loads((root / "plugin.json").read_text(encoding="utf-8"))
    manifest["name"] = "moodle-remote"
    extension = manifest["extensions"]["com.openai"]
    extension["requires_local_executor"] = False
    extension.pop("onboardingSkill", None)
    claude = json.loads((root / ".claude-plugin/plugin.json").read_text(encoding="utf-8"))
    claude["name"] = "moodle-remote"
    config = {"mcpServers": {"moodle": {"type": "streamable-http", "url": url}}}
    portable = {"$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", **config}
    native = {"mcpServers": {"moodle": {"type": "http", "url": url}}}
    entries = {name: (root / name).read_bytes() for name in ("assets/logo.png", "skills/moodle-study/SKILL.md")}
    for name, value in (("plugin.json", manifest), ("mcp.json", portable),
                        (".claude-plugin/plugin.json", claude), (".mcp.json", native)):
        entries[name] = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()
    entries["README.md"] = (
        "# Moodle remoto\n\n"
        "Plugin de consulta, estudio y organización con un MCP remoto y la skill moodle-study.\n\n"
        f"Conector: {url}\n\n"
        "Instala el paquete donde tu cliente admita plugins remotos y completa la conexión OAuth. "
        "Para probar el conector, también puedes añadir esa URL como MCP remoto en tu aplicación. "
        "Introduce tu clave Moodle únicamente en el formulario HTTPS que abre la conexión.\n\n"
        "El paquete no incluye credenciales ni requiere ejecutar Python en tu ordenador. "
        "Los callbacks autorizados en esta beta son los de ChatGPT y Claude web. "
        "La instalación, el uso en móvil y la publicación dependen de las capacidades de cada cliente.\n"
    ).encode()
    output = root / "dist" / "moodle-remote-plugin-0.1.0.zip"
    output.parent.mkdir(exist_ok=True)
    with ZipFile(output, "w", compression=ZIP_DEFLATED) as archive:
        for name, data in entries.items():
            archive.writestr(name, data)
    return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--remote-url", help="Crear un paquete remoto para esta URL HTTPS /mcp.")
    arguments = parser.parse_args()
    print(build_remote(arguments.remote_url) if arguments.remote_url else build())
