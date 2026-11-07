"""Comprueba los paquetes públicos y ejecuta el servidor incluido, sin una cuenta real."""

import hashlib
import json
import platform
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parents[2]
ARCH = {"AMD64": "x86_64", "arm64": "aarch64"}.get(platform.machine(), platform.machine())
PLATFORM = ("windows" if sys.platform == "win32" else "macos") + "-" + ARCH
DIRECTORY = ROOT / "dist/native" / PLATFORM
SERVER = "mudel-ai-connect-server" + (".exe" if sys.platform == "win32" else "")


class PackageTests(unittest.TestCase):
    def test_plugin_contains_only_public_files_and_uses_the_native_launcher(self):
        archives = list(DIRECTORY.glob("mudel-ai-connect-plugin-native-*.zip"))
        self.assertEqual(len(archives), 1)
        with ZipFile(archives[0]) as archive:
            names = {name for name in archive.namelist() if not name.endswith("/") and not name.startswith("__MACOSX/")}
            self.assertEqual(names, {
                "LICENSE", "README.md", "assets/logo.png", "plugin.json", "mcp.json", ".mcp.json",
                ".claude-plugin/plugin.json", f"bin/{SERVER}",
                "skills/setup/SKILL.md", "skills/setup/agents/openai.yaml",
                "skills/mudel-ai-connect-study/SKILL.md", "skills/mudel-ai-connect-study/agents/openai.yaml",
            })
            for name, variable in [("mcp.json", "${PLUGIN_ROOT}"), (".mcp.json", "${CLAUDE_PLUGIN_ROOT}")]:
                entry = json.loads(archive.read(name))["mcpServers"]["mudel-ai-connect"]
                self.assertEqual(entry, {"command": f"{variable}/bin/{SERVER}", "args": ["--plugin"], "type": "stdio"})
            binary = archive.read(f"bin/{SERVER}")
            self.assertGreater(len(binary), 1024)
            if sys.platform != "win32":
                self.assertTrue(archive.getinfo(f"bin/{SERVER}").external_attr >> 16 & 0o111)

    def test_installer_embeds_a_working_server_matching_the_plugin(self):
        plugin = next(DIRECTORY.glob("mudel-ai-connect-plugin-native-*.zip"))
        with ZipFile(plugin) as archive:
            binary = archive.read(f"bin/{SERVER}")
        with tempfile.TemporaryDirectory() as temporary:
            if sys.platform == "win32":
                setup = DIRECTORY / "Mudel-AI-Connect-Setup.exe"
            else:
                with ZipFile(next(DIRECTORY.glob("Mudel-AI-Connect-Setup-*.zip"))) as archive:
                    name = "Mudel AI Connect.app/Contents/MacOS/mudel-ai-connect-setup"
                    setup = Path(archive.extract(name, temporary))
                    setup.chmod(0o700)
                    plist = archive.read("Mudel AI Connect.app/Contents/Info.plist")
                    self.assertIn(b"CFBundleExecutable", plist)
            result = subprocess.run([str(setup), "--package-check"], capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", errors="replace"))
        value = json.loads(result.stdout)
        self.assertEqual(value["server_bytes"], len(binary))
        self.assertEqual(value["server_sha256"], hashlib.sha256(binary).hexdigest())
        self.assertEqual(value["version"], json.loads((ROOT / "rust/plugin/metadata/plugin.json").read_text())["version"])

    def test_checksums_cover_the_deliverables_without_paths_outside_the_package(self):
        sums = (DIRECTORY / "SHA256SUMS.txt").read_text().splitlines()
        self.assertEqual(len(sums), 4)
        for line in sums:
            digest, name = line.split("  ", 1)
            self.assertEqual(Path(name).name, name)
            self.assertEqual(hashlib.sha256((DIRECTORY / name).read_bytes()).hexdigest(), digest)


if __name__ == "__main__":
    if not DIRECTORY.is_dir():
        raise SystemExit("Genera primero los paquetes con rust/scripts/package.ps1")
    unittest.main()
