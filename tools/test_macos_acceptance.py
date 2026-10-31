"""Runner regression tests use synthetic temporary files, NOT game acceptance."""
import importlib.util
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("acceptance", Path(__file__).with_name("macos_acceptance.py"))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_report_requires_success_marker(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            (out / "report.txt").write_text("schema=1\nkind=title\nvideo_driver=cocoa\n")
            with self.assertRaisesRegex(ValueError, "successful final"):
                runner.validate(out, "title")

    def test_dummy_driver_never_passes_preflight(self):
        with tempfile.TemporaryDirectory() as tmp, mock.patch.dict(os.environ, {"SDL_VIDEODRIVER": "dummy"}):
            errors = runner.preflight(Path(tmp), Path(tmp) / "missing-binary")
            self.assertTrue(any("SDL_VIDEODRIVER" in e for e in errors))

    def test_media_cannot_escape_resource_root(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            data = root / "data"
            data.mkdir()
            movie = root / "movie.wmv"
            movie.write_bytes(b"not-real-media")
            with self.assertRaisesRegex(ValueError, "inside selected real data"):
                runner.media_path(data, str(movie), (), ())

    def test_tga_rejects_missing_pixels(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.tga"
            h = bytearray(18)
            h[2], h[16] = 2, 32
            struct.pack_into("<HH", h, 12, 20, 20)
            path.write_bytes(h)
            with self.assertRaisesRegex(ValueError, "incomplete TGA"):
                runner.tga_info(path)

    def test_digest_is_content_addressed(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "fixture"
            p.write_bytes(b"abc")
            self.assertEqual(runner.digest(p)["sha256"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")

    def test_installing_marker_blocks_inventory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".wa2-installing.json").write_text("{}")
            with self.assertRaisesRegex(ValueError, "still in progress"):
                runner.inventory_proof(root)

    def test_manifest_filters_count_only_declared_suffixes(self):
        import json
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "bg").mkdir()
            (root / "bg/a.amp").write_bytes(b"abc")
            (root / "bg/cached.webp").write_bytes(b"cache-not-in-manifest")
            manifest = {"container": "zip-parts", "parts": [{"name": "data.part01.zip", "size": 10, "sha256": "a" * 64}],
                        "inventory": {"files": 1, "bytes": 3, "trees": {"bg": {"files": 1, "bytes": 3}}},
                        "tree_filters": {"bg": {"suffixes": [".amp"]}}}
            (root / ".wa2-release.json").write_text(json.dumps(manifest))
            proof = runner.inventory_proof(root)
            self.assertEqual(proof["trees"]["bg"]["files"], 1)
            (root / "bg/a.amp").write_bytes(b"mismatch")
            with self.assertRaisesRegex(ValueError, "inventory differs"):
                runner.inventory_proof(root)

    def test_failed_process_group_is_cleaned(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = mock.Mock(pid=123456)
            fake.wait.return_value = 124
            with mock.patch.object(runner.subprocess, "Popen", return_value=fake), mock.patch.object(runner, "stop_group") as kill:
                result = runner.invoke(["fake"], {}, Path(tmp), 1)
            self.assertEqual(result["exit_code"], 124)
            kill.assert_called_once_with(123456)


if __name__ == "__main__":
    unittest.main()
