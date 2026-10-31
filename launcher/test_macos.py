#!/usr/bin/env python3
"""Offline launcher regression tests; no native window, builds, or downloads.

Run from the checkout: python3 -B -m unittest discover -s launcher -p test_macos.py
"""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("check_macos", ROOT / "launcher/check_macos.py")
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class CheckerTests(unittest.TestCase):
    def run_check(self, args=(), missing=()):
        output = io.StringIO()
        which = lambda name: None if name in missing else "/existing/bin/" + name
        with mock.patch.object(sys, "platform", "darwin"), \
                mock.patch.object(checker.shutil, "which", side_effect=which), \
                mock.patch.object(checker.ctypes, "CDLL", return_value=object()), \
                contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            status = checker.main(args)
        return status, output.getvalue()

    def test_build_only_does_not_check_data_or_runtime(self):
        with mock.patch.object(checker, "check_data", side_effect=AssertionError("data touched")), \
                mock.patch.object(checker, "library_available", side_effect=AssertionError("library touched")):
            status, text = self.run_check(["--build-only"])
        self.assertEqual(status, 0, text)

    def test_smoke_requires_only_sdl_and_build_tools(self):
        with mock.patch.object(checker, "check_data", side_effect=AssertionError("data touched")), \
                mock.patch.object(checker, "library_available", return_value=True) as library:
            status, text = self.run_check(["--smoke"], missing=("ffmpeg", "ffprobe"))
        self.assertEqual(status, 0, text)
        library.assert_called_once_with("SDL2")

    def test_missing_dependency_advice_is_targeted(self):
        with mock.patch.object(checker, "check_data", return_value=True):
            status, text = self.run_check(missing=("ffprobe",))
        self.assertEqual(status, 1)
        self.assertIn("brew install ffmpeg", text)
        self.assertNotIn("brew install freetype", text)
        self.assertNotIn("brew install rust", text)

    def test_real_data_predicate_and_no_bytecode_or_data_writes(self):
        with tempfile.TemporaryDirectory(prefix="wa2 data fixture ") as temporary:
            data = Path(temporary)
            for tree in ("script", "voice", "bgm", "se", "movie", "font", "bg_2x", "sprite_2x"):
                (data / tree).mkdir()
                (data / tree / "fixture").write_bytes(b"fixture, not playable game data")
            for name in ("1001.bnr", "1001.txt"):
                (data / "script" / name).write_bytes(b"fixture")
            before = {p.relative_to(data): p.stat().st_mtime_ns for p in data.rglob("*")}
            with mock.patch.dict(os.environ, {"WA2_LANG": "jp"}), contextlib.redirect_stdout(io.StringIO()):
                self.assertTrue(checker.check_data(data))
            after = {p.relative_to(data): p.stat().st_mtime_ns for p in data.rglob("*")}
            self.assertEqual(before, after)
            (data / ".wa2-installing.json").write_text("{}")
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertFalse(checker.check_data(data))

    def test_explicit_chinese_requires_chinese_scripts(self):
        with tempfile.TemporaryDirectory() as temporary, \
                mock.patch("fetch_data.runtime_ready", return_value=True), \
                mock.patch.dict(os.environ, {"WA2_LANG": "cn"}), \
                contextlib.redirect_stdout(io.StringIO()):
            self.assertFalse(checker.check_data(Path(temporary)))


class ShellTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="wa2 launcher with spaces ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.checkout = self.root / "source checkout"
        self.checkout.mkdir()
        (self.checkout / "launcher").mkdir()
        shutil.copyfile(ROOT / "run-macos.sh", self.checkout / "run-macos.sh")
        self.bin = self.root / "fake tools"
        self.bin.mkdir()
        self.log = self.root / "commands.jsonl"
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ.get("PATH", ""),
                        TEST_LOG=str(self.log))
        self.tool("uname", "import os\nprint(os.environ.get('TEST_SYSTEM', 'Darwin'))\n")
        self.tool("python3", """import json, os, sys
with open(os.environ['TEST_LOG'], 'a') as log:
    log.write(json.dumps({'tool': 'check', 'args': sys.argv[1:], 'cwd': os.getcwd()}) + '\\n')
sys.exit(int(os.environ.get('TEST_CHECK_STATUS', '0')))
""")
        self.tool("cargo", """import json, os, sys
with open(os.environ['TEST_LOG'], 'a') as log:
    log.write(json.dumps({'tool': 'cargo', 'args': sys.argv[1:], 'cwd': os.getcwd(),
                        'env': dict(os.environ)}) + '\\n')
""")

    def tool(self, name, body):
        path = self.bin / name
        path.write_text("#!{}\n{}".format(sys.executable, body))
        path.chmod(0o755)

    def launch(self, *args):
        result = subprocess.run(["/bin/bash", str(self.checkout / "run-macos.sh"), *args],
                                env=self.env, cwd=self.root, capture_output=True, text=True)
        entries = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        return result, entries

    def test_check_never_compiles_and_uses_no_bytecode_flag(self):
        result, entries = self.launch("--check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([e["tool"] for e in entries], ["check"])
        self.assertEqual(entries[0]["args"], ["-B", "launcher/check_macos.py"])
        self.assertEqual(Path(entries[0]["cwd"]).resolve(), self.checkout.resolve())

    def test_missing_data_or_dependency_stops_before_compile(self):
        self.env["TEST_CHECK_STATUS"] = "1"
        result, entries = self.launch()
        self.assertEqual(result.returncode, 1)
        self.assertEqual([e["tool"] for e in entries], ["check"])

    def test_default_preserves_explicit_environment_and_spaces(self):
        explicit = {"WA2_DATA": "relative data with spaces", "WA2_FONT": "",
                    "WA2_FONT_GAME": "/font with spaces.ttc#2", "WA2_LANG": "jp",
                    "SDL_VIDEODRIVER": "dummy", "SDL_AUDIODRIVER": "dummy",
                    "CARGO_TARGET_DIR": "build with spaces", "RUSTUP_TOOLCHAIN": "custom"}
        self.env.update(explicit)
        result, entries = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([e["tool"] for e in entries], ["check", "cargo"])
        self.assertEqual(entries[1]["args"],
                         ["run", "--offline", "--locked", "--release", "--bin", "wa2-native", "--", "title"])
        for name, value in explicit.items():
            self.assertEqual(entries[1]["env"][name], value)

    def test_build_only_compiles_without_launch(self):
        result, entries = self.launch("--build-only")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(entries[0]["args"][-1], "--build-only")
        self.assertEqual(entries[1]["args"],
                         ["build", "--offline", "--locked", "--release", "--bin", "wa2-native"])

    def test_smoke_uses_existing_diagnostic_without_setting_drivers(self):
        self.env.pop("SDL_VIDEODRIVER", None)
        self.env.pop("SDL_AUDIODRIVER", None)
        result, entries = self.launch("--smoke")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(entries[0]["args"][-1], "--smoke")
        self.assertEqual(entries[1]["args"][-2:], ["--", "fmtinfo"])
        self.assertNotIn("SDL_VIDEODRIVER", entries[1]["env"])
        self.assertNotIn("SDL_AUDIODRIVER", entries[1]["env"])

    def test_scene_is_passed_as_single_numeric_argument(self):
        result, entries = self.launch("1002")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(entries[1]["args"][-3:], ["--", "run", "1002"])

    def test_non_macos_rejected_before_any_check_or_build(self):
        self.env["TEST_SYSTEM"] = "Linux"
        result, entries = self.launch()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(entries, [])

    def test_case_sensitive_image_mounts_only_for_normal_default_launch(self):
        image = self.root / "WA2Data.sparsebundle"
        image.mkdir()
        (self.checkout / "data").symlink_to(self.root / "wa2-data-volume/data")
        self.tool("hdiutil", """import json, os, sys
with open(os.environ['TEST_LOG'], 'a') as log:
    log.write(json.dumps({'tool': 'mount', 'args': sys.argv[1:]}) + '\\n')
""")
        self.env.pop("WA2_DATA", None)
        result, entries = self.launch("--check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([e["tool"] for e in entries], ["check"])
        self.log.unlink()
        result, entries = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([e["tool"] for e in entries], ["mount", "check", "cargo"])
        self.log.unlink()
        self.env["WA2_DATA"] = "external data"
        result, entries = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([e["tool"] for e in entries], ["check", "cargo"])

    def test_bad_arguments_rejected_without_executing_tools(self):
        for args in (("--download-only",), ("title", "1002"), ("1002;touch bad",)):
            result, entries = self.launch(*args)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(entries, [])


if __name__ == "__main__":
    unittest.main()
