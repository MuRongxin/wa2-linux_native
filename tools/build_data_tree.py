#!/usr/bin/env python3
"""Build the loose-file data tree (`data/`) from an installed WA2 + its .pak files.

This is the one-time import step that makes the engine independent of the
original installation: afterwards `wa2-native` only ever reads plain files under
`data/`, never a `.pak` and never the game directory.

    python3 tools/build_data_tree.py --game "/path/to/白色相簿2 整合版"

What it does (all steps are no-ops when the target already exists):

    data/script      <- script.pak            (.bnr bytecode + .txt token pool)
    data/script_cn   <- ck-gal.pak            (CK-GAL Chinese patch text)
    data/bg          <- grp.pak + bak.pak     (backgrounds, incl. *.amp LUTs)
    data/bg_cn       <- <game>/grp/*.tga      (31 Chinese UI override images;
                                               the loose dir wins over grp.pak)
    data/sprite      <- char.pak + IC/char.pak  (both sets merged: their file
                                               names only overlap on 67 entries)
    data/voice       <- VOICE.PAK + IC/VOICE.PAK
    data/bgm         <- BGM.PAK + IC/BGM.PAK
    data/font        <- fnt.pak (Japanese) + fon.pak (Chinese patch) atlases,
                        plus wa2_cell_map_{jp,cn}.tsv from tools/gen_cell_map.py
    data/movie       <- mv*.pak / IC/MV*.pak  (raw ASF -> *.wmv)

Requires `wa2-native` (built) for unpacking: pass --bin to point at it.
"""

import argparse
import os
import shutil
import subprocess
import sys

SUBDIRS = ["script", "script_cn", "bg", "bg_cn", "sprite", "voice", "bgm", "font", "movie"]


def run(cmd):
    print("  $", " ".join(str(c) for c in cmd))
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        print("    ! failed:", (r.stderr or r.stdout).strip()[:200])
    return r.returncode == 0


def unpack(binary, pak, outdir):
    """Unpack one archive; KCAP entries land flat, LAC/unknown are reported."""
    if not os.path.isfile(pak):
        print(f"  (skip, missing) {pak}")
        return False
    os.makedirs(outdir, exist_ok=True)
    return run([binary, "extract", pak, outdir])


def merge(src, dst):
    """Copy everything in src into dst without overwriting existing files."""
    n = 0
    for name in os.listdir(src):
        s, d = os.path.join(src, name), os.path.join(dst, name)
        if os.path.exists(d):
            continue
        if os.path.isfile(s):
            shutil.copy2(s, d)
            n += 1
    return n


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True, help="game installation directory")
    ap.add_argument("--out", default="data", help="data tree root (default: data)")
    ap.add_argument("--bin", default="./target/release/wa2-native")
    ap.add_argument("--tmp", default="/tmp/wa2-import")
    args = ap.parse_args()

    g, out, tmp = args.game, args.out, args.tmp
    if not os.path.isdir(g):
        print("game directory not found:", g, file=sys.stderr)
        return 1
    for d in SUBDIRS:
        os.makedirs(os.path.join(out, d), exist_ok=True)
    print(f"importing from {g}\n         to   {os.path.abspath(out)}\n")

    # --- archives that are already loose in the game directory -------------
    print("[1/6] movies (raw ASF, renamed to .wmv)")
    for name in sorted(os.listdir(g)) + sorted(os.listdir(os.path.join(g, "IC")) if os.path.isdir(os.path.join(g, "IC")) else []):
        if not name.lower().endswith(".pak") or not name.lower().startswith("mv"):
            continue
        src = os.path.join(g, name)
        if not os.path.isfile(src):
            src = os.path.join(g, "IC", name)
        dst = os.path.join(out, "movie", os.path.splitext(name)[0].lower() + ".wmv")
        if not os.path.exists(dst):
            shutil.copyfile(src, dst)
    print("   ", len(os.listdir(os.path.join(out, "movie"))), "movies")

    # --- Chinese UI overrides (loose grp/ wins over grp.pak) ---------------
    print("[2/6] Chinese UI override images (<game>/grp)")
    grp = os.path.join(g, "grp")
    if os.path.isdir(grp):
        for name in os.listdir(grp):
            if name.lower().endswith(".tga"):
                shutil.copy2(os.path.join(grp, name), os.path.join(out, "bg_cn", name))
    print("   ", len(os.listdir(os.path.join(out, "bg_cn"))), "overrides")

    # --- KCAP archives ------------------------------------------------------
    print("[3/6] script.pak + ck-gal.pak")
    unpack(args.bin, os.path.join(g, "script.pak"), os.path.join(out, "script"))
    unpack(args.bin, os.path.join(g, "ck-gal.pak"), os.path.join(out, "script_cn"))

    print("[4/6] grp.pak + bak.pak -> bg")
    for pak in ("grp.pak", "bak.pak"):
        t = os.path.join(tmp, pak.split(".")[0])
        if unpack(args.bin, os.path.join(g, pak), t):
            merge(t, os.path.join(out, "bg"))
            shutil.rmtree(t, ignore_errors=True)
    print("   ", len(os.listdir(os.path.join(out, "bg"))), "backgrounds")

    print("[5/6] char.pak + IC/char.pak -> sprite (merged)")
    for pak, tag in ((os.path.join(g, "char.pak"), "char"), (os.path.join(g, "IC", "char.pak"), "char_ic")):
        t = os.path.join(tmp, tag)
        if unpack(args.bin, pak, t):
            merge(t, os.path.join(out, "sprite"))
            shutil.rmtree(t, ignore_errors=True)
    print("   ", len(os.listdir(os.path.join(out, "sprite"))), "sprites")

    print("[6/6] VOICE.PAK + BGM.PAK (+ IC variants)")
    for pak, dst in (("VOICE.PAK", "voice"), ("IC/VOICE.PAK", "voice"), ("BGM.PAK", "bgm"), ("IC/BGM.PAK", "bgm")):
        t = os.path.join(tmp, dst + "_" + os.path.basename(os.path.dirname(pak) or "x"))
        if unpack(args.bin, os.path.join(g, pak), t):
            merge(t, os.path.join(out, dst))
            shutil.rmtree(t, ignore_errors=True)
    for d in ("voice", "bgm"):
        print("   ", d, len(os.listdir(os.path.join(out, d))), "files")

    # --- fonts + cell maps --------------------------------------------------
    print("[7/7] font atlases (fnt.pak = Japanese, fon.pak = Chinese patch)")
    for pak, atlas_name in (("fnt.pak", "jp_body80.tga"), ("fon.pak", "cn_body80.tga")):
        t = os.path.join(tmp, pak.split(".")[0])
        if not unpack(args.bin, os.path.join(g, pak), t):
            continue
        # The 80px sheet is the 3200x3840 one; pick by decoded size.
        for name in os.listdir(t):
            p = os.path.join(t, name)
            if name.endswith("80.tga") and os.path.getsize(p) > 10_000_000:
                shutil.copy2(p, os.path.join(out, "font", atlas_name))
        shutil.rmtree(t, ignore_errors=True)
    # Cell maps come from the exes' own font tables.
    subprocess.run([sys.executable, os.path.join(os.path.dirname(__file__), "gen_cell_map.py"),
                    "--game", g], check=False)
    for name in ("wa2_cell_map_jp.tsv", "wa2_cell_map_cn.tsv"):
        src = os.path.join("tools", "tables", name)
        if os.path.isfile(src):
            shutil.copy2(src, os.path.join(out, "font", name))

    print("\nDone. Run:  ./run.sh          (Chinese by default)")
    print("            WA2_LANG=jp ./run.sh   (Japanese)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
