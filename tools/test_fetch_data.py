"""Offline integration checks: python3 -m unittest discover -s tools -p test_fetch_data.py"""

import contextlib
import hashlib
import http.server
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import unittest
from unittest import mock
import urllib.parse
import zipfile

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("fetch_data", ROOT / "tools/fetch_data.py")
fetch = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fetch)


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="wa2 installer ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.dest = self.root / "project"
        self.cache = self.root / "cache"
        self.cache.mkdir()
        self.requests = []
        self.ignore_range = False
        self.disconnect = False
        self.paths = ["script/1001.bnr", "script/1001.txt", "script_cn/1001.bnr", "script_cn/1001.txt",
                      "voice/line.ogg", "bgm/BGM_031_A.OGG", "se/SE_8905.WAV", "movie/mv.wmv",
                      "font/cn_body80.tga", "bg/sepia.AMP"]
        for scale in (2, 4, 8):
            self.paths += [f"bg_{scale}x/T0000.webp", f"sprite_{scale}x/char.webp", f"bg_cn_{scale}x/sys.webp"]
        self.files = {"data/" + p: (p + " fixture\n").encode() for p in self.paths}
        self.blobs = {}
        parts = []
        entries = list(self.files.items())
        for index, chunk in enumerate((entries[:8], entries[8:]), 1):
            name = f"data-v1.1.0.part{index:03d}.zip"
            output = io.BytesIO()
            with zipfile.ZipFile(output, "w") as archive:
                for path, content in chunk:
                    archive.writestr(path, content)
            blob = output.getvalue()
            self.blobs[name] = blob
            parts.append({"name": name, "size": len(blob), "sha256": hashlib.sha256(blob).hexdigest()})
        trees = {}
        for path, content in self.files.items():
            tree = path.split("/")[1]
            info = trees.setdefault(tree, {"files": 0, "bytes": 0})
            info["files"] += 1
            info["bytes"] += len(content)
        self.manifest = {"container": "zip-parts", "version": "data-v1.1.0", "parts": parts,
                         "tree_filters": {"bg": {"suffixes": [".amp"]}},
                         "inventory": {"trees": trees, "files": len(self.files),
                                       "bytes": sum(len(v) for v in self.files.values())}}
        self.blobs["manifest.json"] = json.dumps(self.manifest).encode()
        test = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_GET(self):
                path = urllib.parse.urlsplit(self.path).path
                test.requests.append((path, dict(self.headers)))
                if path.endswith("/releases/tags/data-v1.1.0"):
                    body = json.dumps({"assets_url": test.base + "/assets", "assets": []}).encode()
                elif path == "/assets":
                    body = json.dumps(test.assets).encode()
                else:
                    body = test.blobs.get(path.rsplit("/", 1)[-1])
                    if body is None:
                        self.send_error(404)
                        return
                offset = 0
                if self.headers.get("Range") and not test.ignore_range:
                    offset = int(self.headers["Range"].split("=")[1].split("-")[0])
                    self.send_response(206)
                    self.send_header("Content-Range", f"bytes {offset}-{len(body)-1}/{len(body)}")
                else:
                    self.send_response(200)
                self.send_header("Content-Length", str(len(body) - offset))
                self.end_headers()
                if test.disconnect and path.endswith("part001.zip"):
                    test.disconnect = False
                    self.wfile.write(body[:len(body)//2])
                    self.wfile.flush()
                    self.connection.shutdown(socket.SHUT_RDWR)
                else:
                    self.wfile.write(body[offset:])

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base = f"http://127.0.0.1:{self.server.server_port}"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.close_server)
        self.assets = [{"name": name, "size": len(blob), "state": "uploaded",
                        "url": self.base + "/download/" + name,
                        "browser_download_url": self.base + "/download/" + name}
                       for name, blob in self.blobs.items()]
        self.log = io.StringIO()
        self.redirect = contextlib.redirect_stdout(self.log)
        self.redirect.__enter__()
        self.addCleanup(self.redirect.__exit__, None, None, None)
        patch = mock.patch.object(fetch, "API", self.base)
        patch.start()
        self.addCleanup(patch.stop)

    def close_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def install(self, *extra):
        return fetch.main(["--dest", str(self.dest), "--parts-dir", str(self.cache),
                           "--repo", "test/game", "--tag", "data-v1.1.0", "--proxy", "", "--token", "", *extra])

    def downloads(self):
        return [(p, h) for p, h in self.requests if p.endswith(".zip")]

    def test_install_preserves_saves_and_next_launch_is_offline(self):
        save = self.dest / "data/save/slot.dat"
        save.parent.mkdir(parents=True)
        save.write_bytes(b"my save")
        config = self.dest / "data/wa2.ini"
        config.write_bytes(b"my settings")
        self.assertEqual(self.install("--ensure", "--remove-parts"), 0)
        for path, content in self.files.items():
            self.assertEqual((self.dest / path).read_bytes(), content)
        self.assertEqual(save.read_bytes(), b"my save")
        self.assertEqual(config.read_bytes(), b"my settings")
        self.assertFalse((self.dest / "data" / fetch.INSTALLING).exists())
        self.assertEqual(list(self.cache.glob("*.zip")), [])
        count = len(self.requests)
        self.assertEqual(self.install("--ensure"), 0)
        self.assertEqual(len(self.requests), count)
        self.assertIn("总进度 100.0%", self.log.getvalue())
        self.assertIn("解压资源 100.0%", self.log.getvalue())

    def test_missing_or_unfinished_volume_stops_before_download(self):
        self.assets[-2]["state"] = "starter"
        with self.assertRaisesRegex(fetch.FetchError, "尚未准备齐全"):
            self.install("--ensure")
        self.assertEqual(self.downloads(), [])
        self.assertFalse((self.dest / "data").exists())

    def test_missing_manifest_stops_before_download(self):
        self.assets = [a for a in self.assets if a["name"] != "manifest.json"]
        with self.assertRaisesRegex(fetch.FetchError, "manifest.json"):
            self.install("--ensure")
        self.assertEqual(self.downloads(), [])

    def test_resume_partial_download(self):
        part = self.manifest["parts"][0]
        (self.cache / (part["name"] + ".part")).write_bytes(self.blobs[part["name"]][:100])
        self.install("--ensure")
        self.assertEqual(self.downloads()[0][1]["Range"], "bytes=100-")
        self.assertEqual((self.cache / part["name"]).read_bytes(), self.blobs[part["name"]])

    def test_server_ignoring_range_does_not_append_full_response(self):
        self.ignore_range = True
        self.test_resume_partial_download()
        self.assertIn("不支持续传", self.log.getvalue())

    def test_connection_break_is_resumed(self):
        self.disconnect = True
        with mock.patch.object(fetch.time, "sleep"):
            self.install("--ensure")
        requests = self.downloads()
        self.assertGreater(len(requests), 2)
        self.assertIn("Range", requests[1][1])

    def test_corrupt_cache_of_correct_size_is_replaced(self):
        part = self.manifest["parts"][0]
        (self.cache / part["name"]).write_bytes(b"x" * part["size"])
        self.install("--ensure")
        self.assertEqual((self.cache / part["name"]).read_bytes(), self.blobs[part["name"]])

    def test_bad_hash_blocks_extraction_and_keeps_install_marker(self):
        part = self.manifest["parts"][0]
        self.blobs[part["name"]] = b"x" * part["size"]
        with mock.patch.object(fetch.time, "sleep"), self.assertRaisesRegex(fetch.FetchError, "SHA-256"):
            self.install("--ensure")
        self.assertTrue((self.dest / "data" / fetch.INSTALLING).is_file())
        self.assertFalse((self.dest / "data/script/1001.bnr").exists())

    def test_interrupted_extraction_retries_offline_despite_core_files(self):
        self.install("--ensure")
        (self.dest / "data" / fetch.INSTALLING).write_text("{}")
        staging = self.dest / "data" / fetch.UNPACK_DIR
        staging.mkdir()
        (staging / "file-orphan").write_bytes(b"interrupted write")
        count = len(self.requests)
        self.assertEqual(self.install("--ensure", "--check-only"), 0)
        self.assertEqual(len(self.requests), count)
        self.assertFalse((self.dest / "data" / fetch.INSTALLING).exists())
        self.assertFalse(staging.exists())

    def test_network_failure_automatically_uses_complete_cached_parts(self):
        self.install("--ensure")
        (self.dest / "data" / fetch.INSTALLING).write_text("{}")
        with mock.patch.object(fetch, "find_release", side_effect=fetch.urllib.error.URLError("offline")):
            self.assertEqual(self.install("--ensure"), 0)
        self.assertFalse((self.dest / "data" / fetch.INSTALLING).exists())
        self.assertIn("使用完整的本地缓存", self.log.getvalue())

    def test_custom_data_directory(self):
        custom = self.root / "custom resources"
        self.install("--ensure", "--data-dir", str(custom))
        self.assertTrue(fetch.runtime_ready(custom))
        self.assertFalse((self.dest / "data").exists())

    def test_archive_cannot_write_outside_tree_or_through_symlinks(self):
        data = self.dest / "data"
        data.mkdir(parents=True)
        part = self.manifest["parts"][0]
        for name in ("data/../outside", "/tmp/outside", "data/save/slot.dat"):
            with self.subTest(name=name):
                with zipfile.ZipFile(self.cache / part["name"], "w") as archive:
                    archive.writestr(name, b"bad")
                with self.assertRaises(fetch.FetchError):
                    fetch.extract_parts(self.cache, data, self.manifest)
        (data / "save").mkdir()
        (data / "script").symlink_to(data / "save", target_is_directory=True)
        with zipfile.ZipFile(self.cache / part["name"], "w") as archive:
            archive.writestr("data/script/slot.dat", b"bad")
        with self.assertRaisesRegex(fetch.FetchError, "符号链接"):
            fetch.extract_parts(self.cache, data, self.manifest)
        self.assertFalse((data / "save/slot.dat").exists())

    def test_token_bypasses_proxy(self):
        sources = fetch.asset_sources(self.assets[0], "test-secret", ["https://proxy.invalid/", "https://second.invalid/"])
        self.assertEqual(len(sources), 1)
        self.assertEqual(sources[0][0], self.assets[0]["url"])
        self.assertEqual(sources[0][1]["Authorization"], "Bearer test-secret")

    def test_proxy_failure_falls_back_to_direct_and_preserves_range(self):
        part = self.manifest["parts"][0]
        (self.cache / (part["name"] + ".part")).write_bytes(self.blobs[part["name"]][:100])
        actual_open = fetch.urllib.request.urlopen
        proxy_requests = []

        def open_request(request, **kwargs):
            if request.full_url.startswith("https://proxy.invalid/"):
                proxy_requests.append(request)
                raise fetch.urllib.error.HTTPError(request.full_url, 404, "Unavailable", {}, None)
            return actual_open(request, **kwargs)

        with mock.patch.object(fetch.urllib.request, "urlopen", side_effect=open_request):
            self.install("--ensure", "--proxy", "https://proxy.invalid/")
        self.assertTrue(proxy_requests)
        self.assertTrue(all(not r.has_header("Authorization") for r in proxy_requests))
        self.assertEqual(self.downloads()[0][1]["Range"], "bytes=100-")

    def test_multiple_proxies_fail_over_before_direct(self):
        part = self.manifest["parts"][0]
        (self.cache / (part["name"] + ".part")).write_bytes(self.blobs[part["name"]][:100])
        actual_open = fetch.urllib.request.urlopen
        visited = []

        def open_request(request, **kwargs):
            url = request.full_url
            if url.startswith("https://bad.invalid/"):
                visited.append("bad")
                raise fetch.urllib.error.HTTPError(url, 404, "Unavailable", {}, None)
            if url.startswith("https://good.invalid/"):
                visited.append("good")
                direct = fetch.urllib.request.Request(url[len("https://good.invalid/"):], headers=dict(request.headers))
                return actual_open(direct, **kwargs)
            if "/download/" in url:
                self.fail("Should have used the working second proxy before direct download")
            return actual_open(request, **kwargs)

        with mock.patch.object(fetch.urllib.request, "urlopen", side_effect=open_request):
            self.install("--ensure", "--proxies", "https://bad.invalid/,https://good.invalid/")
        self.assertIn("bad", visited)
        self.assertIn("good", visited)
        self.assertEqual(self.downloads()[0][1]["Range"], "bytes=100-")

    def test_slow_proxy_switches_and_resumes(self):
        actual_open = fetch.urllib.request.urlopen

        def open_request(request, **kwargs):
            if request.full_url.startswith("https://slow.invalid/"):
                direct = fetch.urllib.request.Request(request.full_url[len("https://slow.invalid/"):],
                                                     headers=dict(request.headers))
                response = actual_open(direct, **kwargs)
                original_read = response.read1
                response.read1 = lambda limit: original_read(min(limit, 64))
                return response
            return actual_open(request, **kwargs)

        with mock.patch.object(fetch.urllib.request, "urlopen", side_effect=open_request), \
                mock.patch.object(fetch, "PROXY_SPEED_WINDOW", 0), \
                mock.patch.object(fetch, "PROXY_MIN_SPEED", float("inf")):
            self.install("--ensure", "--proxy", "https://slow.invalid/")
        self.assertEqual(self.downloads()[1][1]["Range"], "bytes=64-")
        self.assertIn("持续过慢", self.log.getvalue())

    def test_proxy_ignoring_range_switches_instead_of_losing_partial_file(self):
        part = self.manifest["parts"][0]
        (self.cache / (part["name"] + ".part")).write_bytes(self.blobs[part["name"]][:100])
        actual_open = fetch.urllib.request.urlopen

        def open_request(request, **kwargs):
            if request.full_url.startswith("https://no-range.invalid/"):
                headers = {key: value for key, value in request.headers.items() if key.lower() != "range"}
                direct = fetch.urllib.request.Request(request.full_url[len("https://no-range.invalid/"):], headers=headers)
                return actual_open(direct, **kwargs)
            return actual_open(request, **kwargs)

        with mock.patch.object(fetch.urllib.request, "urlopen", side_effect=open_request):
            self.install("--ensure", "--proxy", "https://no-range.invalid/")
        self.assertEqual(self.downloads()[1][1]["Range"], "bytes=100-")
        self.assertIn("当前代理不支持续传", self.log.getvalue())

    def test_low_disk_space_stops_before_download(self):
        disk = shutil._ntuple_diskusage(total=100, used=100, free=0)
        with mock.patch.object(fetch.shutil, "disk_usage", return_value=disk):
            with self.assertRaisesRegex(fetch.FetchError, "磁盘空间不足"):
                self.install("--ensure")
        self.assertEqual(self.downloads(), [])
        self.assertFalse((self.dest / "data" / fetch.INSTALLING).exists())

    def launcher(self):
        tools = self.dest / "tools"
        tools.mkdir(parents=True)
        shutil.copy2(ROOT / "run.sh", self.dest / "run.sh")
        shutil.copy2(ROOT / "tools/prepare_appimage.sh", tools / "prepare_appimage.sh")
        (tools / "appimage").mkdir()
        shutil.copy2(ROOT / "tools/appimage/release.conf", tools / "appimage/release.conf")
        source = (ROOT / "tools/fetch_data.py").read_text().replace(
            'API = "https://api.github.com"', 'API = ' + repr(self.base))
        (tools / "fetch_data.py").write_text(source)
        shutil.copy2(ROOT / "tools/check_runtime.py", tools / "check_runtime.py")
        fakebin = self.root / "fakebin"
        fakebin.mkdir()
        cargo = fakebin / "cargo"
        cargo.write_text("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'cargo test'; exit 0; fi\n"
                         "mkdir -p target/release\n"
                         "cat > target/release/wa2-native <<'ENGINE'\n#!/bin/sh\n"
                         "printf '%s\\n' \"$@\" > launch-args\n"
                         "printf '%s' \"$WA2_DATA\" > launch-data\nENGINE\n"
                         "chmod +x target/release/wa2-native\n")
        cargo.chmod(0o755)
        rustc = fakebin / "rustc"
        rustc.write_text("#!/bin/sh\necho 'rustc test'\n")
        rustc.chmod(0o755)
        env = dict(os.environ, PATH=str(fakebin) + os.pathsep + os.environ["PATH"],
                   WA2_DATA=str(self.root / "custom data"), WA2_DATA_PROXY="", GITHUB_TOKEN="",
                   WA2_AUTO_INSTALL="0", WA2_BUILD_FROM_SOURCE="1", WA2_DATA_PROXIES="", WA2_DATA_REPO="test/game", WA2_DATA_TAG="data-v1.1.0", WA2_DATA_CACHE=str(self.cache))
        command = ["bash", str(self.dest / "run.sh")]
        return command, env

    def test_launcher_bootstraps_builds_and_forwards_scene(self):
        command, env = self.launcher()
        run = subprocess.run(command + ["1002"], cwd="/", env=env, text=True, capture_output=True, timeout=15)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertEqual((self.dest / "launch-args").read_text(), "run\n1002\n")
        self.assertEqual((self.dest / "launch-data").read_text(), env["WA2_DATA"])
        count = len(self.requests)
        run = subprocess.run(command, cwd="/", env=env, text=True, capture_output=True, timeout=15)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertEqual((self.dest / "launch-args").read_text(), "title\n")
        self.assertEqual(len(self.requests), count)

    def test_launcher_check_is_read_only(self):
        command, env = self.launcher()
        run = subprocess.run(command + ["--check"], cwd="/", env=env, text=True, capture_output=True, timeout=15)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertTrue(self.requests)
        self.assertEqual(self.downloads(), [])
        self.assertFalse(Path(env["WA2_DATA"]).exists())
        self.assertFalse((self.dest / "target/release/wa2-native").exists())

    def test_launcher_missing_runtime_dependency_stops_before_downloading(self):
        command, env = self.launcher()
        stubs = self.root / "stubs"
        stubs.mkdir()
        (stubs / "ctypes.py").write_text("def CDLL(name):\n    raise OSError('missing library')\n")
        env["PYTHONPATH"] = str(stubs)
        run = subprocess.run(command, cwd="/", env=env, text=True, capture_output=True, timeout=15)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn("SDL2", run.stderr)
        self.assertEqual(self.requests, [])
        self.assertFalse((self.dest / "launch-args").exists())
        # Download-only mode is also useful on a machine without game libraries.
        run = subprocess.run(command + ["--download-only"], cwd="/", env=env, text=True, capture_output=True, timeout=15)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertTrue(fetch.runtime_ready(Path(env["WA2_DATA"])))
        self.assertFalse((self.dest / "target/release/wa2-native").exists())

    def test_launcher_help_and_invalid_arguments_do_not_download(self):
        command, env = self.launcher()
        run = subprocess.run(command + ["--help"], env=env, text=True, capture_output=True, timeout=15)
        self.assertEqual(run.returncode, 0)
        self.assertIn("--download-only", run.stdout)
        run = subprocess.run(command + ["--unknown"], env=env, text=True, capture_output=True, timeout=15)
        self.assertEqual(run.returncode, 2)
        self.assertEqual(self.requests, [])

    def test_launcher_prefers_appimage_without_system_python(self):
        command, env = self.launcher()
        binary = self.dest / "bin/wa2-native-linux-x86_64.AppImage"
        binary.parent.mkdir()
        binary.write_text("#!/bin/sh\nmkdir -p squashfs-root/usr/bin\n"
                          "cat > squashfs-root/AppRun <<'ENTRY'\n#!/bin/sh\n"
                          "printf '%s\\n' \"$@\" > launch-args\n"
                          "printf '%s' \"$WA2_PROJECT_ROOT\" > launch-root\nENTRY\n"
                          "printf '#!/bin/sh\\nexit 0\\n' > squashfs-root/usr/bin/wa2-native\n"
                          "chmod +x squashfs-root/AppRun squashfs-root/usr/bin/wa2-native\n")
        binary.chmod(0o755)
        fake_python = self.root / "fakebin/python3"
        fake_python.write_text("#!/bin/sh\necho 'Unexpected system Python call' >&2\nexit 91\n")
        fake_python.chmod(0o755)
        env["WA2_BUILD_FROM_SOURCE"] = "0"
        env["WA2_APPIMAGE_SHA256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        env["WA2_APPIMAGE_SIZE"] = str(binary.stat().st_size)
        run = subprocess.run(command + ["1002"], cwd="/", env=env, text=True, capture_output=True, timeout=15)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertEqual((self.dest / "launch-args").read_text(), "1002\n")
        self.assertEqual((self.dest / "launch-root").read_text(), str(self.dest))
        self.assertEqual(self.requests, [])


if __name__ == "__main__":
    unittest.main()
