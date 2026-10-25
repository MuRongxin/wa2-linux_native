"""Local bootstrap tests with a synthetic executable; no game or network."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class BootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="wa2 bootstrap with spaces ")
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.project = self.work / "project"
        config = self.project / "tools/appimage"
        config.mkdir(parents=True)
        shutil.copy2(ROOT / "tools/prepare_appimage.sh", self.project / "tools")
        shutil.copy2(ROOT / "run.sh", self.project)
        self.fakebin = self.work / "fakebin"
        self.fakebin.mkdir()
        self.payload = self.work / "payload"
        self.payload.write_text("""#!/bin/sh
if [ "${TEST_EXTRACT_FAIL:-0}" = 1 ]; then exit 1; fi
printf 'extract\\n' >> "$TEST_EXTRACT_LOG"
mkdir -p squashfs-root/usr/bin
cat > squashfs-root/AppRun <<'ENTRY'
#!/bin/sh
printf '%s\\n' "$@" > "$TEST_LAUNCH_LOG"
printf '%s' "$WA2_PROJECT_ROOT" > "$TEST_ROOT_LOG"
exit 0
ENTRY
printf '#!/bin/sh\\nexit 0\\n' > squashfs-root/usr/bin/wa2-native
chmod +x squashfs-root/AppRun squashfs-root/usr/bin/wa2-native
""")
        self.digest = hashlib.sha256(self.payload.read_bytes()).hexdigest()
        (config / "release.conf").write_text(
            "APPIMAGE_RELEASE_REPO=test/game\nAPPIMAGE_RELEASE_TAG=app-v0.1.0\n"
            f"APPIMAGE_RELEASE_SHA256={self.digest}\n"
            f"APPIMAGE_RELEASE_SIZE={self.payload.stat().st_size}\nAPPIMAGE_UNPACKED_SIZE=1\n")
        curl = self.fakebin / "curl"
        curl.write_text(f"#!{sys.executable}\n" + """
import json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ["TEST_REQUEST_LOG"], "a") as log:
    log.write(json.dumps(args) + "\\n")
if "--head" in args:
    raise SystemExit(0)
if os.environ.get("TEST_NETWORK_FAIL") == "1":
    raise SystemExit(28)
dest = pathlib.Path(args[args.index("--output") + 1])
data = pathlib.Path(os.environ["TEST_PAYLOAD"]).read_bytes()
if os.environ.get("TEST_BAD_HASH") == "1":
    data = b"X" * len(data)
offset = dest.stat().st_size if dest.exists() and "--continue-at" in args else 0
if os.environ.get("TEST_INTERRUPT") == "1" and not dest.exists():
    dest.write_bytes(data[:len(data)//2])
    raise SystemExit(18)
with dest.open("ab" if offset else "wb") as target:
    target.write(data[offset:])
""")
        curl.chmod(0o755)
        python = self.fakebin / "python3"
        python.write_text("#!/bin/sh\necho 'System Python must not be used' >&2\nexit 91\n")
        python.chmod(0o755)
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("WA2_", "TEST_")) and k not in ("APPDIR", "APPIMAGE")}
        self.env.update(PATH=str(self.fakebin) + os.pathsep + os.environ["PATH"],
                        WA2_APPIMAGE_PROXIES="", WA2_AUTO_INSTALL="0",
                        TEST_PAYLOAD=str(self.payload), TEST_REQUEST_LOG=str(self.work / "requests"),
                        TEST_EXTRACT_LOG=str(self.work / "extracts"), TEST_LAUNCH_LOG=str(self.work / "launch"),
                        TEST_ROOT_LOG=str(self.work / "root"))
        self.image = self.project / "bin/wa2-native-linux-x86_64.AppImage"
        self.cache = self.project / "bin/runtime" / self.digest

    def run_script(self, script="tools/prepare_appimage.sh", *args):
        return subprocess.run(["bash", str(self.project / script), *args], cwd="/",
                              env=self.env, capture_output=True, text=True, timeout=15)

    def requests(self):
        path = self.work / "requests"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def assert_ok(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_full_launcher_downloads_once_without_python_and_reuses_runtime(self):
        self.assert_ok(self.run_script("run.sh", "1002"))
        self.assertEqual((self.work / "launch").read_text(), "1002\n")
        self.assertEqual((self.work / "root").read_text(), str(self.project))
        self.assertEqual(self.image.read_bytes(), self.payload.read_bytes())
        self.assertEqual((self.cache / ".complete").read_text().strip(), self.digest)
        count = len(self.requests())
        self.env["TEST_NETWORK_FAIL"] = "1"
        self.assert_ok(self.run_script("run.sh"))
        self.assertEqual(len(self.requests()), count)
        self.assertEqual((self.work / "extracts").read_text(), "extract\n")

    def test_corrupt_download_never_replaces_old_image_or_executes(self):
        self.image.parent.mkdir()
        self.image.write_bytes(b"previous version")
        self.env["TEST_BAD_HASH"] = "1"
        result = self.run_script()
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
        self.assertEqual(self.image.read_bytes(), b"previous version")
        self.assertFalse((self.work / "extracts").exists())

    def test_interrupted_download_resumes_on_next_route(self):
        self.env.update(TEST_INTERRUPT="1", WA2_APPIMAGE_PROXIES="https://proxy.example/")
        self.assert_ok(self.run_script())
        self.assertEqual(len(self.requests()), 2)
        self.assertTrue(self.requests()[0][-1].startswith("https://proxy.example/"))
        self.assertTrue(self.requests()[1][-1].startswith("https://github.com/"))
        self.assertEqual(self.image.read_bytes(), self.payload.read_bytes())

    def test_check_does_not_download_create_bin_or_extract(self):
        self.assert_ok(self.run_script("tools/prepare_appimage.sh", "--check"))
        self.assertIn("--head", self.requests()[0])
        self.assertFalse(self.image.parent.exists())
        self.assertFalse((self.work / "extracts").exists())

    def test_download_can_resume_after_restarting_launcher(self):
        self.env["TEST_INTERRUPT"] = "1"
        result = self.run_script()
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
        parts = list((self.project / "bin/.downloads").glob("*.part"))
        self.assertEqual(len(parts), 1)
        self.assertEqual(parts[0].stat().st_size, self.payload.stat().st_size // 2)
        self.assert_ok(self.run_script())
        self.assertEqual(self.image.read_bytes(), self.payload.read_bytes())

    def test_missing_cached_engine_is_repaired(self):
        self.assert_ok(self.run_script())
        (self.cache / "squashfs-root/usr/bin/wa2-native").unlink()
        count = len(self.requests())
        self.assert_ok(self.run_script())
        self.assertEqual(len(self.requests()), count)
        self.assertTrue((self.cache / "squashfs-root/usr/bin/wa2-native").is_file())

    def test_invalid_digest_cannot_escape_cache_directory(self):
        self.env["WA2_APPIMAGE_SHA256"] = "../../outside"
        result = self.run_script()
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
        self.assertFalse(self.image.parent.exists())
        self.assertEqual(self.requests(), [])

    def test_failed_extraction_never_marks_cache_complete(self):
        self.env["TEST_EXTRACT_FAIL"] = "1"
        result = self.run_script()
        self.assertEqual(result.returncode, 78, result.stdout + result.stderr)
        self.assertFalse((self.cache / ".complete").exists())
        self.assertEqual(list(self.cache.parent.glob(".unpack.*")), [])
        self.env["TEST_EXTRACT_FAIL"] = "0"
        self.assert_ok(self.run_script())
        self.assertTrue((self.cache / ".complete").is_file())

    def test_network_failure_does_not_install_or_build(self):
        installer = self.project / "tools/install_runtime.sh"
        installer.write_text('#!/bin/sh\ntouch "$TEST_ROOT_LOG"\nexit 99\n')
        self.env.update(TEST_NETWORK_FAIL="1", WA2_AUTO_INSTALL="1")
        result = self.run_script("run.sh")
        self.assertEqual(result.returncode, 75, result.stdout + result.stderr)
        self.assertFalse((self.work / "root").exists())
        self.assertFalse((self.project / "target").exists())

    def test_old_glibc_is_classified_as_incompatible_before_download(self):
        getconf = self.fakebin / "getconf"
        getconf.write_text("#!/bin/sh\necho 'glibc 2.27'\n")
        getconf.chmod(0o755)
        result = self.run_script()
        self.assertEqual(result.returncode, 78, result.stdout + result.stderr)
        self.assertEqual(self.requests(), [])


if __name__ == "__main__":
    unittest.main()
