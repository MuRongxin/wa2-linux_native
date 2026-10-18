#!/usr/bin/env python3
"""Group event CGs by what the artwork actually is, not by file name.

The trees name a picture's variants with adjacent ids (v200800, v200801, ...),
which reads like "one picture plus its 差分" - but that is an assumption from
the naming scheme, not a fact. Some of those ids are separate pictures, and some
ids far apart are the same picture reused.

Two passes, because one pass is not enough:

  1. cluster  - every CG is decoded to a small grayscale thumbnail and grouped
                by cosine distance. Small is where similar-looking frames get
                caught, but it also calls "almost the same" identical.
  2. verify   - each candidate group is decoded again at logical resolution and
                re-scored by mean absolute pixel difference, so a group is only
                kept when the pictures really are the same frame.

Usage:
    python3 tools/cg_dedup_cluster.py --scene 2031 2313
    python3 tools/cg_dedup_cluster.py --stems v200800 v200801 --verify 1280x720
    python3 tools/cg_dedup_cluster.py --scene 2031 --cluster-threshold 0.02 --mad 2.0
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent / "data"
CACHE = Path(__file__).resolve().parent.parent / "data" / "cache" / "cg_thumbs"


def parse_pam(data: bytes) -> np.ndarray:
    if not data.startswith(b"P7" + bytes([10])):
        raise ValueError("not a PAM")
    end = data.index(b"ENDHDR" + bytes([10])) + 7
    width = height = depth = 0
    for line in data[:end].decode("latin-1").splitlines():
        parts = line.split()
        if len(parts) < 2:
            continue
        if parts[0] == "WIDTH":
            width = int(parts[1])
        elif parts[0] == "HEIGHT":
            height = int(parts[1])
        elif parts[0] == "DEPTH":
            depth = int(parts[1])
    if depth != 4:
        raise ValueError("unexpected PAM depth")
    return np.frombuffer(data[end : end + width * height * 4], dtype=np.uint8).reshape(height, width, 4)


def luma(rgba: np.ndarray) -> np.ndarray:
    return (
        rgba[..., 0].astype(np.float32) * 0.299
        + rgba[..., 1].astype(np.float32) * 0.587
        + rgba[..., 2].astype(np.float32) * 0.114
    )


def decode(path: Path, size) -> np.ndarray:
    """Decode and resample inside libwebp; never materialises the 59 MP source."""
    with tempfile.NamedTemporaryFile(suffix=".pam", delete=False) as handle:
        temp = Path(handle.name)
    try:
        proc = subprocess.run(
            ["dwebp", "-quiet", "-pam", "-resize", str(size[0]), str(size[1]), str(path), "-o", str(temp)],
            capture_output=True,
        )
        if proc.returncode != 0:
            raise RuntimeError("dwebp failed on " + str(path))
        rgba = parse_pam(temp.read_bytes())
    finally:
        temp.unlink(missing_ok=True)
    return luma(rgba)


def load(names, size) -> dict:
    """Grayscale thumbnails, memoised on disk under data/cache/cg_thumbs."""
    CACHE.mkdir(parents=True, exist_ok=True)
    key = "%dx%d" % size
    out = {}
    missing = []
    for name in names:
        path = None
        for tree in ("bg_4x", "bg_8x"):
            candidate = ROOT / tree / (name + ".webp")
            if candidate.is_file():
                path = candidate
                break
        if path is None:
            print("  missing " + name, file=sys.stderr)
            continue
        stamp = "%s_%s_%s.npy" % (key, tree, name)
        cached = CACHE / stamp
        if cached.is_file():
            out[name] = np.load(cached)
            continue
        out[name] = decode(path, size)
        np.save(cached, out[name])
        missing.append(name)
    if missing:
        print("  decoded %d new thumbnails at %s" % (len(missing), key), file=sys.stderr)
    return out


def cosine_distance(names, images) -> np.ndarray:
    matrix = np.stack([images[n].ravel() for n in names]).astype(np.float32)
    norm = matrix / np.linalg.norm(matrix, axis=1, keepdims=True)
    return 1.0 - norm @ norm.T


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--stems", nargs="*", default=[])
    parser.add_argument("--scene", nargs="*", default=[])
    parser.add_argument("--cluster-size", default="160x90")
    parser.add_argument("--cluster-threshold", type=float, default=0.02)
    parser.add_argument("--verify-size", default="1280x720")
    parser.add_argument("--mad", type=float, default=2.0, help="mean abs pixel diff below which a group survives verification")
    args = parser.parse_args()

    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from cg_scenes import scene_cgs

    stems = list(args.stems)
    for scene in args.scene:
        stems.extend(scene_cgs(scene))
    stems = sorted(dict.fromkeys(stems))
    if not stems:
        parser.error("give --stems or --scene")

    cluster_size = tuple(int(v) for v in args.cluster_size.split("x"))
    verify_size = tuple(int(v) for v in args.verify_size.split("x"))

    print("pass 1: %d images, cosine distance on %dx%d grayscale" % (len(stems), *cluster_size))
    small = load(stems, cluster_size)
    names = sorted(small)
    distance = cosine_distance(names, small)
    pairs = distance[np.triu_indices(len(names), 1)]
    print("  distance quantiles: p0.1%%=%.4f p1%%=%.4f p5%%=%.4f median=%.4f" % (
        np.quantile(pairs, 0.001), np.quantile(pairs, 0.01), np.quantile(pairs, 0.05), np.median(pairs)))

    from scipy.cluster.hierarchy import average, fcluster
    from scipy.spatial.distance import squareform

    groups = fcluster(average(squareform(distance, checks=False)), t=args.cluster_threshold, criterion="distance")
    members = {int(g): [n for n, gi in zip(names, groups) if gi == g] for g in set(int(x) for x in groups)}
    multi = {g: v for g, v in members.items() if len(v) > 1}
    print("  %d candidate groups (%d with more than one member, %d singletons)" % (len(members), len(multi), len(members) - len(multi)))

    print("pass 2: split candidates at %dx%d by mean absolute pixel difference" % verify_size)
    big = load(sorted(n for v in multi.values() for n in v), verify_size)
    from scipy.cluster.hierarchy import fcluster, single
    from scipy.spatial.distance import squareform

    final: list[list[str]] = []
    for g in sorted(multi, key=lambda g: -len(multi[g])):
        group = multi[g]
        mad = np.zeros((len(group), len(group)), dtype=np.float32)
        for i in range(len(group)):
            for j in range(i + 1, len(group)):
                mad[i, j] = mad[j, i] = float(np.abs(big[group[i]] - big[group[j]]).mean())
        sub = fcluster(single(squareform(mad, checks=False)), t=args.mad, criterion="distance")
        for s in sorted({int(x) for x in sub}):
            part = [n for n, si in zip(group, sub) if si == s]
            worst = max(
                (mad[group.index(a), group.index(b)] for a in part for b in part if a != b),
                default=0.0,
            )
            print("  [%d] %-46s intra MAD %.2f" % (len(part), " ".join(part), worst))
            final.append(part)
    for group in members.values():
        if len(group) == 1:
            final.append(group)
    final.sort(key=lambda grp: (-len(grp), grp[0]))
    print()
    print("result: %d distinct pictures out of %d files" % (len(final), len(names)))
    for group in final:
        if len(group) > 1:
            print("  [%d] %s" % (len(group), " ".join(group)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
