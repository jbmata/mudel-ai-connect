"""Ejecuta las pruebas de shared y local: python run_tests.py [-v]"""

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent

if __name__ == "__main__":
    suite = unittest.TestSuite()
    for app in ("shared", "local"):
        folder = str(ROOT / app / "tests")
        suite.addTests(unittest.defaultTestLoader.discover(folder, top_level_dir=folder))
    result = unittest.TextTestRunner(verbosity=2 if "-v" in sys.argv else 1).run(suite)
    sys.exit(not result.wasSuccessful())
