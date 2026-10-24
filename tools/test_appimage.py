"""Exercise the actual shipped runtime: python3 -m unittest discover -s tools -p test_appimage.py"""

import base64
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
IMAGE = ROOT / "bin/wa2-native-linux-x86_64.AppImage"


@unittest.skipUnless(IMAGE.is_file(), "Build the AppImage first")
class AppImageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix="wa2 packaged runtime ")
        cls.addClassCleanup(cls.temp.cleanup)
        cls.work = Path(cls.temp.name)
        env = dict(os.environ)
        env.pop("APPIMAGE_EXTRACT_AND_RUN", None)
        run = subprocess.run([str(IMAGE), "--appimage-extract"], cwd=cls.work, env=env,
                             capture_output=True, text=True, timeout=60)
        if run.returncode:
            raise RuntimeError(run.stderr)
        cls.appdir = cls.work / "squashfs-root"

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="wa2 runtime checks ")
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.env = {key: value for key, value in os.environ.items() if not key.startswith(("WA2_", "PYTHON"))}
        self.env.update(WA2_PROJECT_ROOT=str(self.work), WA2_DATA=str(self.work / "data"),
                        WA2_INI=str(self.work / "settings.ini"), PYTHONHOME="/not-installed", PYTHONPATH="/not-installed",
                        LD_LIBRARY_PATH=str(self.appdir / "python/lib") + ":" + str(self.appdir / "usr/lib"),
                        SSL_CERT_FILE=str(self.appdir / "usr/share/wa2-native/ca-certificates.crt"))

    def run_package(self, *args):
        return subprocess.run([str(self.appdir / "AppRun"), *args], env=self.env,
                              capture_output=True, text=True, timeout=45)

    def run_tool(self, name, *args):
        return subprocess.run([str(self.appdir / "usr/bin" / name), *map(str, args)], env=self.env,
                              capture_output=True, timeout=45)

    def test_artifact_checksum_and_metadata(self):
        digest = hashlib.sha256(IMAGE.read_bytes()).hexdigest()
        recorded = IMAGE.with_suffix(IMAGE.suffix + ".sha256").read_text().split()[0]
        self.assertEqual(digest, recorded)
        info = json.loads((self.appdir / "usr/share/wa2-native/build-info.json").read_text())
        self.assertFalse(info["includes_game_data"])
        self.assertFalse((self.appdir / "data").exists())
        self.assertTrue((self.appdir / "usr/share/wa2-native/sources/ffmpeg-9.0.2.tar.xz").is_file())

    def test_embedded_python_ignores_host_configuration(self):
        result = self.run_package("--check-runtime")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("内置", result.stdout)
        self.assertIn("内置字体包: 已找到", result.stdout)
        self.assertIn(str(self.appdir / "usr/bin/ffmpeg"), result.stdout)
        self.assertFalse((self.work / "data").exists())

    def test_help_does_not_download_resources(self):
        result = self.run_package("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--download-only", result.stdout)
        self.assertFalse((self.work / "data").exists())

    def test_bundled_font_renders_without_game_assets(self):
        output = self.work / "font sample.tga"
        result = self.run_package("--engine", "font-sample", str(output))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("3 of 3 font(s) verified", result.stderr)
        image = output.read_bytes()
        self.assertEqual(struct.unpack_from("<HH", image, 12), (1280, 720))
        self.assertGreater(len(set(image[18:])), 30)

    def test_media_tools_include_game_codecs(self):
        result = self.run_tool("ffmpeg", "-hide_banner", "-decoders")
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        for codec in (b"wmv3", b"wmav2", b"h264", b"aac", b"vorbis", b"webp"):
            self.assertIn(codec, result.stdout)
        result = self.run_tool("ffprobe", "-version")
        self.assertEqual(result.returncode, 0, result.stderr.decode())

    def test_image_fallback_produces_rgba_pam(self):
        # An original 2x2 bitmap, not a game asset. Bottom-up BGR rows + padding.
        pixels = b"\x00\x00\xff\x00\xff\x00\0\0\xff\0\0\xff\xff\xff\0\0"
        header = struct.pack("<2sIHHI", b"BM", 54 + len(pixels), 0, 0, 54)
        dib = struct.pack("<IiiHHIIiiII", 40, 2, 2, 1, 24, 0, len(pixels), 0, 0, 0, 0)
        image = self.work / "colors.bmp"
        image.write_bytes(header + dib + pixels)
        result = self.run_tool("ffmpeg", "-v", "error", "-i", image, "-frames:v", "1",
                               "-f", "image2pipe", "-c:v", "pam", "-pix_fmt", "rgba", "-")
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        metadata, rgba = result.stdout.split(b"ENDHDR\n", 1)
        self.assertIn(b"DEPTH 4", metadata)
        self.assertEqual(len(rgba), 16)

    def test_packaged_audio_and_video_helpers_on_local_media(self):
        samples = [ROOT / "data/bgm/BGM_003_A.OGG", ROOT / "data/se/SE_8160.WAV",
                   ROOT / "data/voice/1002_0000_10.OGG", ROOT / "data/movie/mv080.wmv",
                   ROOT / "data/movie_2x/mv080.wmv"]
        if not all(path.is_file() for path in samples):
            self.skipTest("User-owned media is not installed on this build machine")
        for path in samples:
            with self.subTest(path=path):
                result = self.run_tool("ffmpeg", "-hide_banner", "-loglevel", "error", "-i", path,
                                       "-t", "0.25", "-f", "s16le", "-ar", "48000", "-ac", "2", "-")
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertGreater(len(result.stdout), 0)
        for path in samples[-2:]:
            result = self.run_tool("ffmpeg", "-v", "error", "-i", path, "-frames:v", "1", "-an",
                                   "-vf", "scale=160:90", "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "rgba", "-")
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            self.assertEqual(len(result.stdout), 160 * 90 * 4)


if __name__ == "__main__":
    unittest.main()
