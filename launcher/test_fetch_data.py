#!/usr/bin/env python3
"""Offline parallel-download tests: python3 -B -m unittest discover -s launcher -p test_fetch_data.py"""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("fetch_data_parallel_tests", Path(__file__).with_name("fetch_data.py"))
fetch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fetch)


def fixtures(count):
    parts = [{"name": f"data.part{i:03}.zip", "size": 16, "sha256": "a" * 64}
             for i in range(count)]
    assets = {p["name"]: dict(p, url="https://api.github.com/asset/1",
                             browser_download_url="https://github.com/asset/1") for p in parts}
    manifest = {"container": "zip-parts", "parts": parts,
                "inventory": {"files": 1, "bytes": 1, "trees": {"script": {"files": 1, "bytes": 1}}}}
    return parts, assets, manifest


class ParallelDownloadTests(unittest.TestCase):
    def test_worker_limit_and_every_part_downloaded_once(self):
        for jobs in (1, 2, 3, 4):
            with self.subTest(jobs=jobs):
                parts, assets, _ = fixtures(jobs * 2)
                barrier = threading.Barrier(jobs)
                lock = threading.Lock()
                seen = []
                active = peak = 0

                def download(asset, target, token, sha, proxies, completed, total, label, *, stop):
                    nonlocal active, peak
                    with lock:
                        active += 1
                        peak = max(peak, active)
                        seen.append(target.name)
                    self.assertEqual(asset, assets[target.name])
                    self.assertEqual(token, "test-token")
                    self.assertEqual(sha, "a" * 64)
                    self.assertEqual(proxies, ["https://example.com/"])
                    self.assertEqual((completed, total), (0, 16))
                    self.assertFalse(stop.is_set())
                    barrier.wait(timeout=5)
                    with lock:
                        active -= 1

                with mock.patch.object(fetch, "download_asset", side_effect=download), \
                        mock.patch.object(fetch, "say"):
                    fetch.download_parts(parts, assets, Path("unused"), "test-token",
                                         ["https://example.com/"], jobs)
                self.assertEqual(peak, jobs)
                self.assertCountEqual(seen, [p["name"] for p in parts])
                self.assertEqual(len(seen), len(set(seen)))

    def test_failure_cancels_work_without_scheduling_remaining_parts(self):
        parts, assets, _ = fixtures(12)
        barrier = threading.Barrier(4)
        seen = []
        lock = threading.Lock()
        error = fetch.FetchError("fixture failure")

        def download(asset, target, *args, stop):
            with lock:
                seen.append(target.name)
            barrier.wait(timeout=5)
            if target.name == parts[0]["name"]:
                raise error
            self.assertTrue(stop.wait(timeout=5), "running workers were not cancelled")
            raise fetch.CancelledError()

        with mock.patch.object(fetch, "download_asset", side_effect=download), \
                mock.patch.object(fetch, "say"):
            with self.assertRaises(fetch.FetchError) as raised:
                fetch.download_parts(parts, assets, Path("unused"), "", [], 4)
        self.assertIs(raised.exception, error)
        self.assertCountEqual(seen, [p["name"] for p in parts[:4]])

    def test_interrupt_cancels_workers_before_return(self):
        parts, assets, _ = fixtures(8)
        stops = []
        finished = []
        lock = threading.Lock()

        def download(*args, stop):
            with lock:
                stops.append(stop)
            self.assertTrue(stop.wait(timeout=5))
            with lock:
                finished.append(True)
            raise fetch.CancelledError()

        with mock.patch.object(fetch, "download_asset", side_effect=download), \
                mock.patch.object(fetch, "say"), \
                mock.patch.object(fetch, "wait", side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                fetch.download_parts(parts, assets, Path("unused"), "", [], 4)
        self.assertTrue(stops)
        self.assertTrue(all(stop.is_set() for stop in stops))
        self.assertEqual(len(stops), len(finished))

    def test_cancelled_stream_retains_partial_bytes(self):
        parts, assets, _ = fixtures(1)
        stop = threading.Event()
        response = mock.MagicMock()
        response.__enter__.return_value = response
        response.status = 200
        chunks = iter([b"first", b"second"])

        def read(_):
            chunk = next(chunks)
            if chunk == b"second":
                stop.set()
            return chunk

        response.read1.side_effect = read
        with tempfile.TemporaryDirectory() as temporary, \
                mock.patch.object(fetch.urllib.request, "urlopen", return_value=response), \
                mock.patch.object(fetch, "say"), mock.patch.object(fetch, "Progress"):
            target = Path(temporary) / parts[0]["name"]
            with self.assertRaises(fetch.CancelledError):
                fetch.download_asset(assets[target.name], target, "token", "a" * 64, stop=stop)
            self.assertEqual(target.with_suffix(".zip.part").read_bytes(), b"first")
            self.assertFalse(target.exists())

    def test_invalid_jobs_rejected_before_network_or_files(self):
        for jobs in ("0", "5", "-1", "abc", "1.5"):
            with self.subTest(jobs=jobs), \
                    mock.patch.object(fetch, "find_release") as release, \
                    mock.patch.object(fetch, "runtime_ready") as ready, \
                    contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as raised:
                    fetch.main(["--jobs", jobs])
                self.assertEqual(raised.exception.code, 2)
                release.assert_not_called()
                ready.assert_not_called()


class InstallerDownloadTests(unittest.TestCase):
    def run_installer(self, temporary, download, args=()):
        parts, assets, manifest = fixtures(8)
        release = {"assets": list(assets.values()) + [{"name": "manifest.json", "size": 1}]}
        with mock.patch.object(fetch, "find_release", return_value=release), \
                mock.patch.object(fetch, "asset_content", return_value=json.dumps(manifest).encode()), \
                mock.patch.object(fetch, "download_asset", side_effect=download), \
                mock.patch.object(fetch, "check_space") as space, \
                mock.patch.object(fetch, "extract_parts") as extract, \
                mock.patch.object(fetch, "verify_manifest_files", return_value=True), \
                mock.patch.object(fetch, "say"):
            try:
                result = fetch.main(["--dest", temporary, "--token", "fixture-token", "--proxy", "", *args])
            except BaseException:
                extract.assert_not_called()
                space.assert_called_once()
                self.assertTrue((Path(temporary) / "data" / fetch.INSTALLING).exists())
                raise
            extract.assert_called_once()
            space.assert_called_once()
            self.assertFalse((Path(temporary) / "data" / fetch.INSTALLING).exists())
            return result, parts

    def test_default_serial_order_and_progress_unchanged(self):
        calls = []

        def download(*args, **kwargs):
            calls.append((threading.current_thread(), args, kwargs))

        with tempfile.TemporaryDirectory() as temporary:
            result, parts = self.run_installer(temporary, download)
        self.assertEqual(result, 0)
        self.assertEqual([args[1].name for _, args, _ in calls], [p["name"] for p in parts])
        for index, (thread, args, kwargs) in enumerate(calls):
            self.assertIs(thread, threading.main_thread())
            self.assertEqual(args[2], "fixture-token")
            self.assertEqual(args[5:7], (index * 16, len(parts) * 16))
            self.assertEqual(kwargs, {})

    def test_parallel_extract_only_after_all_downloads_complete(self):
        completed = []
        lock = threading.Lock()

        def download(asset, target, *args, **kwargs):
            with lock:
                completed.append(target.name)

        # run_installer asserts extraction exactly once after successful return;
        # also inspect completion count at the extraction boundary itself.
        original_patch = mock.patch.object
        with tempfile.TemporaryDirectory() as temporary:
            parts, assets, manifest = fixtures(8)
            release = {"assets": list(assets.values()) + [{"name": "manifest.json", "size": 1}]}
            with original_patch(fetch, "find_release", return_value=release), \
                    original_patch(fetch, "asset_content", return_value=json.dumps(manifest).encode()), \
                    original_patch(fetch, "download_asset", side_effect=download), \
                    original_patch(fetch, "check_space"), original_patch(fetch, "say"), \
                    original_patch(fetch, "verify_manifest_files", return_value=True), \
                    original_patch(fetch, "extract_parts", side_effect=lambda *a: self.assertEqual(len(completed), 8)) as extract:
                self.assertEqual(fetch.main(["--dest", temporary, "--jobs", "4", "--proxy", ""]), 0)
                extract.assert_called_once()
        self.assertCountEqual(completed, [p["name"] for p in parts])

    def test_failed_download_never_extracts(self):
        for jobs in (1, 4):
            with self.subTest(jobs=jobs), tempfile.TemporaryDirectory() as temporary:
                error = fetch.FetchError("fixture failure")
                with self.assertRaises(fetch.FetchError) as raised:
                    self.run_installer(temporary, mock.Mock(side_effect=error), ["--jobs", str(jobs)])
                self.assertIs(raised.exception, error)

    def test_authenticated_sources_stay_direct(self):
        _, assets, _ = fixtures(1)
        asset = next(iter(assets.values()))
        sources = fetch.asset_sources(asset, "fixture-token", ["https://example.com/"])
        self.assertEqual(len(sources), 1)
        self.assertEqual(sources[0][0], asset["url"])
        self.assertEqual(sources[0][1]["Authorization"], "Bearer fixture-token")


class ArchiveCaseCollisionTests(unittest.TestCase):
    def check_names(self, groups, sensitive=False):
        _, _, manifest = fixtures(len(groups))
        archives = []
        for names in groups:
            archive = mock.MagicMock()
            archive.__enter__.return_value = archive
            archive.infolist.return_value = [fetch.zipfile.ZipInfo(name) for name in names]
            archives.append(archive)
        with mock.patch.object(fetch, "filesystem_case_sensitive", return_value=sensitive), \
                mock.patch.object(fetch.zipfile, "ZipFile", side_effect=archives) as zip_open:
            fetch.check_archive_case_collisions(Path("cache"), Path("data"), manifest)
            return zip_open.call_count

    def test_case_only_names_across_parts_are_rejected(self):
        for extension in ("tga", "webp"):
            with self.subTest(extension=extension):
                names = [f"data/bg_2x/T0010.{extension}", f"data/bg_2x/t0010.{extension}"]
                with self.assertRaises(fetch.FetchError) as raised:
                    self.check_names([[names[0]], [names[1]]])
                for expected in (*names, "APFS", "--data-dir", "WA2_DATA", "--check-only"):
                    self.assertIn(expected, str(raised.exception))

    def test_directory_case_aliases_are_rejected(self):
        with self.assertRaises(fetch.FetchError):
            self.check_names([["data/bg_2x/Art/first.tga", "data/bg_2x/art/second.tga"]])

    def test_identical_names_and_repeated_directories_preserve_existing_behavior(self):
        name = "data/bg_2x/T0010.tga"
        self.assertEqual(self.check_names([["data/bg_2x/", name], ["data/bg_2x/", name]]), 2)

    def test_sensitive_filesystem_does_not_reject_or_scan_archives(self):
        self.assertEqual(self.check_names([["data/bg_2x/T0010.tga", "data/bg_2x/t0010.tga"]], True), 0)

    def test_probe_cleans_up_its_only_temporary_file(self):
        for same_file in (False, True):
            with self.subTest(same_file=same_file), tempfile.TemporaryDirectory() as temporary:
                data = Path(temporary)
                marker = data / "existing"
                marker.write_bytes(b"unchanged")
                with mock.patch.object(Path, "samefile", return_value=same_file):
                    self.assertEqual(fetch.filesystem_case_sensitive(data), not same_file)
                self.assertEqual(list(data.iterdir()), [marker])
                self.assertEqual(marker.read_bytes(), b"unchanged")
        with tempfile.TemporaryDirectory() as temporary:
            data = Path(temporary)
            with mock.patch.object(Path, "samefile", side_effect=FileNotFoundError):
                self.assertTrue(fetch.filesystem_case_sensitive(data))
            self.assertEqual(list(data.iterdir()), [])

    def test_probe_cleans_up_on_unexpected_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            data = Path(temporary)
            with mock.patch.object(Path, "samefile", side_effect=PermissionError):
                with self.assertRaises(PermissionError):
                    fetch.filesystem_case_sensitive(data)
            self.assertEqual(list(data.iterdir()), [])

    def test_collision_preflight_precedes_any_extraction_writes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            data, cache = root / "data", root / "cache"
            data.mkdir()
            cache.mkdir()
            _, _, manifest = fixtures(2)
            for part, name in zip(manifest["parts"], ["T0010.tga", "t0010.tga"]):
                with fetch.zipfile.ZipFile(cache / part["name"], "w") as archive:
                    archive.writestr("data/script/" + name, b"fixture bytes")
            with mock.patch.object(fetch, "filesystem_case_sensitive", return_value=False):
                with self.assertRaises(fetch.FetchError):
                    fetch.extract_parts(cache, data, manifest)
            self.assertEqual(list(data.iterdir()), [], "no staging or member files may be written")


if __name__ == "__main__":
    unittest.main()
