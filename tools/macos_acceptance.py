#!/usr/bin/env python3
"""Bounded, local-resource Mac title/story/media acceptance. No downloads or UI automation.

A pass is a representative real-runtime checkpoint, NOT an end-to-end playthrough,
asset authenticity guarantee, save-UI test, or proof of audibility.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import struct
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return {"path": str(path.resolve()), "bytes": path.stat().st_size, "sha256": h.hexdigest()}


def parse_report(path):
    result = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            result[key] = value
    return result


def tga_info(path):
    with path.open("rb") as stream:
        header = stream.read(18)
    if len(header) != 18:
        raise ValueError("truncated TGA: " + str(path))
    width, height = struct.unpack_from("<HH", header, 12)
    if not width or not height or header[2] != 2 or header[16] not in (24, 32):
        raise ValueError("unexpected screenshot TGA: " + str(path))
    expected = 18 + header[0] + width * height * (header[16] // 8)
    if path.stat().st_size < expected:
        raise ValueError("incomplete TGA pixels: " + str(path))
    return dict(digest(path), width=width, height=height)


def stop_group(pid):
    # The child is always started in its own session. Kill descendants even if
    # the watchdog already exited, leaving its decoder in the old process group.
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def invoke(command, env, out, timeout):
    start = time.monotonic()
    with (out / "stdout.log").open("wb") as stdout, (out / "stderr.log").open("wb") as stderr:
        child = subprocess.Popen(command, cwd=str(ROOT), env=env, stdin=subprocess.DEVNULL,
                                 stdout=stdout, stderr=stderr, start_new_session=True)
        try:
            code = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            stop_group(child.pid)
            child.wait()
            code = 124
        except BaseException:
            stop_group(child.pid)
            child.wait()
            raise
        # Also reap leftover decoder group members on normal/failure exits.
        stop_group(child.pid)
    return {"command": command, "exit_code": code, "elapsed_seconds": round(time.monotonic() - start, 3)}


def regular_nonempty(path):
    return path.is_file() and path.stat().st_size > 0


def preflight(data, binary):
    errors = []
    if platform.system() != "Darwin":
        errors.append("actual Mac acceptance requires macOS")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        errors.append("build native binary first: " + str(binary))
    for key, expected in (("SDL_VIDEODRIVER", "cocoa"), ("SDL_AUDIODRIVER", "coreaudio")):
        if key in os.environ and os.environ[key] != expected:
            errors.append(key + " must be unset or " + expected + "; no dummy/software fallback")
    for name in ("1002", "2001", "3002"):
        for suffix in ("bnr", "txt"):
            path = data / "script_cn" / (name + "." + suffix)
            if not regular_nonempty(path):
                errors.append("missing Chinese scene resource: " + str(path))
            elif suffix == "bnr":
                with path.open("rb") as stream:
                    if stream.read(4) != b"LSCR":
                        errors.append("not real LSCR bytecode: " + str(path))
    for name in ("cn_body80.tga", "wa2_cell_map_cn.tsv"):
        if not regular_nonempty(data / "font" / name):
            errors.append("missing bitmap font resource: " + name)
    if not (data / "script").is_dir():
        errors.append("missing original script directory required by Data::resolve")
    if not any(all((data / (name + suffix)).is_dir() for name in ("bg", "bg_cn", "sprite"))
               for suffix in ("_4x", "_8x", "_2x", "")):
        errors.append("no complete background/Chinese-background/sprite sibling tree")
    for tool in ("ffmpeg", "ffprobe"):
        import shutil
        if not shutil.which(tool):
            errors.append("missing media executable: " + tool)
    return errors


def inventory_proof(data):
    """Reuse only offline validation/counting; never enter downloader main()."""
    import importlib.util
    if (data / ".wa2-installing.json").exists():
        raise ValueError("resource installation is still in progress")
    path = data / ".wa2-release.json"
    manifest = json.loads(path.read_text(encoding="utf-8"))
    spec = importlib.util.spec_from_file_location("wa2_acceptance_resource_manifest", ROOT / "launcher/fetch_data.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.validate_manifest(manifest)
    trees = {}
    for tree, expected in manifest["inventory"]["trees"].items():
        files, size = module.tree_totals(data, tree, manifest)
        trees[tree] = {"files": files, "bytes": size, "expected": expected}
        if files != expected["files"] or size != expected["bytes"]:
            raise ValueError("resource inventory differs: " + tree + " " + str(trees[tree]))
    return {"manifest": digest(path), "trees": trees,
            "scope": "filtered count/byte inventory; ZIP SHA-256 validation belongs to downloader log"}


def media_path(data, given, trees, extensions):
    if given:
        candidate = Path(given).expanduser().resolve()
        if not regular_nonempty(candidate):
            raise ValueError("media input is absent/empty: " + str(candidate))
        if data not in candidate.parents:
            raise ValueError("acceptance media must be inside selected real data tree")
        return candidate
    for tree in trees:
        directory = data / tree
        if directory.is_dir():
            files = sorted(p for p in directory.iterdir() if p.suffix.lower() in extensions and regular_nonempty(p))
            if files:
                return files[0].resolve()
    raise ValueError("no local media found in " + ", ".join(trees))


def validate(out, kind):
    report = parse_report(out / "report.txt")
    if report.get("schema") != "1" or report.get("kind") != kind or report.get("status") != "pass":
        raise ValueError("missing successful final runtime report")
    if report.get("video_driver") != "cocoa" or report.get("audio_driver") != "coreaudio":
        raise ValueError("native Cocoa/CoreAudio drivers not proven")
    if int(report.get("renderer_nonuniform_pixels", "0")) < 1000:
        raise ValueError("SDL renderer did not demonstrate nonuniform content")
    names = {"title": ("title", "renderer"), "scene": ("scene", "restored", "continued", "renderer"),
             "movie": ("movie", "renderer"), "load": ("restored", "restored-renderer", "continued")}[kind]
    screenshots = {name: tga_info(out / (name + ".tga")) for name in names}
    if kind in ("title", "scene", "load"):
        if int(report.get("chinese_characters", "0")) < 1 or report.get("missing_glyphs") != "0":
            raise ValueError("Chinese rendering evidence incomplete")
    if kind == "scene":
        for key in ("save_equal", "restored_pixels_equal", "continuation"):
            if report.get(key) != "true":
                raise ValueError("save/load checkpoint absent: " + key)
        if (out / "roundtrip.sav").read_bytes() != (out / "roundtrip-reencoded.sav").read_bytes():
            raise ValueError("on-disk save roundtrip differs")
        if screenshots["scene"]["sha256"] != screenshots["restored"]["sha256"]:
            raise ValueError("restored screenshot differs")
    elif kind == "load":
        for key in ("external_restore", "restored_pixels_equal", "continuation"):
            if report.get(key) != "true":
                raise ValueError("fresh-process save/load checkpoint absent: " + key)
    else:
        if int(report.get("decoded_samples", "0")) <= 0 or int(report.get("queued_bytes", "0")) <= 0:
            raise ValueError("real decoded/queued audio evidence absent")
    if kind == "movie" and (int(report.get("frames", "0")) < 2 or report.get("changing_frames") != "true"):
        raise ValueError("changing decoded video frames not proven")
    return report, screenshots


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", default=os.environ.get("WA2_DATA", str(ROOT / "data")))
    parser.add_argument("--binary", default=str(ROOT / "target/release/wa2-native"))
    parser.add_argument("--output", required=True, help="new evidence directory (must not exist)")
    parser.add_argument("--movie", help="real movie inside data tree; default first movie by filename")
    parser.add_argument("--audio", help="real OGG inside data tree; default first BGM by filename")
    parser.add_argument("--timeout", type=int, default=120, help="per-checkpoint deadline 10..600 seconds")
    parser.add_argument("--movie-seconds", type=float, default=12.0, help="1..60 seconds; must reach visible moving frames")
    args = parser.parse_args(argv)
    if not 10 <= args.timeout <= 600 or not 1 <= args.movie_seconds <= 60:
        parser.error("timeout must be 10..600; movie-seconds must be 1..60")
    data, binary, output = (Path(p).expanduser().resolve() for p in (args.data, args.binary, args.output))
    if output.exists():
        parser.error("output already exists; choose a fresh path to prevent stale evidence")
    output.mkdir(parents=True)
    result = {"schema": 1, "started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "platform": platform.platform(), "data": str(data), "status": "blocked", "checkpoints": [],
              "scope": "Real-resource representative checkpoints, not a full playthrough or audible-speaker proof",
              "environment": {k: v for k, v in os.environ.items() if k.startswith(("WA2_", "SDL_"))}}
    summary = output / "summary.json"
    def save_summary():
        summary.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    errors = preflight(data, binary)
    if errors:
        result["errors"] = errors
        save_summary()
        print("BLOCKED: " + "; ".join(errors), file=sys.stderr)
        return 2
    try:
        result["inventory_evidence"] = inventory_proof(data)
    except Exception as error:
        result["errors"] = ["resource inventory blocked: " + str(error)]
        save_summary()
        print("BLOCKED: " + str(error), file=sys.stderr)
        return 2
    try:
        movie = media_path(data, args.movie, ("movie_8x", "movie_4x", "movie_2x", "movie"), (".wmv", ".pak", ".mp4"))
        audio = media_path(data, args.audio, ("bgm",), (".ogg",))
        voice = media_path(data, None, ("voice",), (".ogg",))
        resources = [data / "script_cn" / (scene + suffix) for scene in ("1002", "2001", "3002") for suffix in (".bnr", ".txt")]
        resources += [data / "font/cn_body80.tga", data / "font/wa2_cell_map_cn.tsv", movie, audio, voice]
        for path in (data / "script/Global.vrb", data / "script_cn/Global.vrb"):
            if path.is_file():
                resources.append(path)
        result["resource_evidence"] = [digest(p) for p in resources]
        result["binary"] = digest(binary)
        # Deterministic isolated settings, avoiding fullscreen and muted user INI.
        ini = output / "acceptance.ini"
        ini.write_text("win_disp=0\nwait=1\nall_vol=70\nbgm_vol=70\nse_vol=70\nvoice_vol=70\n", encoding="utf-8")
        env = dict(os.environ, WA2_DATA=str(data), WA2_LANG="cn", WA2_INI=str(ini),
                   WA2_ACCEPTANCE_TIMEOUT=str(args.timeout))
        # Existing unattended knobs must not select another title submenu.
        env.pop("WA2_TITLE_AUTO", None)
        cases = [("title", "title", [])]
        for label, scene in (("ic", "1002"), ("cc", "2001"), ("coda", "3002")):
            cases += [(label, "scene", [scene]),
                      (label + "-reload", "load", [str(output / label / "roundtrip.sav")])]
        cases += [("movie", "movie", [str(movie)])]
        result["status"] = "running"
        save_summary()
        for label, kind, extra in cases:
            out = output / label
            out.mkdir()
            case_ini = out / "acceptance.ini"
            case_ini.write_bytes(ini.read_bytes())
            env["WA2_INI"] = str(case_ini)
            command = [str(binary), "acceptance-" + kind] + extra + [str(out)]
            if kind == "movie":
                command += [str(args.movie_seconds)]
            entry = dict(invoke(command, env, out, args.timeout + 5), name=label, status="fail")
            result["checkpoints"].append(entry)
            try:
                if entry["exit_code"]:
                    raise ValueError("runtime failed; inspect stderr.log")
                entry["report"], entry["screenshots"] = validate(out, kind)
                entry["status"] = "pass"
            except (OSError, ValueError) as error:
                entry["error"] = str(error)
            save_summary()
            print(label + ": " + entry["status"])
        for label, source in (("audio", audio), ("voice", voice)):
            out = output / label
            out.mkdir()
            entry = dict(invoke([str(binary), "audio-test", str(source), "3"], env, out, args.timeout + 5), name=label, status="fail")
            import re
            text = (out / "stdout.log").read_text(encoding="utf-8", errors="replace")
            match = re.search(r"audio-test done: driver=coreaudio pulled (\d+) samples, queued (\d+) bytes", text)
            if entry["exit_code"] == 0 and match and all(int(v) > 0 for v in match.groups()):
                entry["status"] = "pass"
            result["checkpoints"].append(entry)
            save_summary()
        result["status"] = "pass" if all(c["status"] == "pass" for c in result["checkpoints"]) else "fail"
    except (OSError, ValueError) as error:
        result["errors"] = [str(error)]
        result["status"] = "fail"
    save_summary()
    print(result["status"].upper() + ": " + str(summary))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
