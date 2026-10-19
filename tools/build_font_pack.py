#!/usr/bin/env python3
"""Build/verify the bundled font pack in `fonts/`.

The design (docs/字体设置功能设计方案.md §1.1) asks for a CJK font shipped with
the program, its licence next to it, and an integrity manifest with size and
SHA-256. This script does all three:

    python3 tools/build_font_pack.py            # copy from the system + write manifest
    python3 tools/build_font_pack.py --check    # verify the manifest against disk
    python3 tools/build_font_pack.py --from DIR # copy the .ttc files from DIR

The coverage column is the share of characters actually used by the Chinese
patch scripts that the face has a glyph for. That is the number the design
wants on record before a font is shipped, and the runtime rejects a bundled
font whose manifest hash does not match the file on disk.
"""
import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONT_DIR = os.path.join(ROOT, "fonts")
LICENSE_DIR = os.path.join(FONT_DIR, "LICENSES")
MANIFEST = os.path.join(FONT_DIR, "manifest.tsv")
PACK_VERSION = 1

# (file name, face index, expected family) -- the family FreeType reports.
PACK = [
    ("NotoSansCJK-Medium.ttc", 2, "Noto Sans CJK SC"),
    ("NotoSansCJK-Regular.ttc", 2, "Noto Sans CJK SC"),
]

SYSTEM_FONT_DIRS = [
    "/usr/share/fonts/opentype/noto",
    "/usr/share/fonts/truetype/noto",
    "/usr/local/share/fonts/noto",
]
LICENSE_SOURCES = [
    "/usr/share/doc/fonts-noto-cjk/copyright",
    "/usr/share/doc/fonts-noto-cjk/LICENSE",
]


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def script_corpus():
    """Distinct characters the Chinese-patch scripts actually use.

    Mirrors how src/codec/sjis.rs decodes them: the SJIS code goes through the
    CN table (cn_text.rs) when present, otherwise CP932. Only the .txt string
    pools are read; the bytecode holds no text.
    """
    sjis = {}
    path = os.path.join(ROOT, "src", "codec", "sjis_table.rs")
    if os.path.exists(path):
        txt = open(path, encoding="utf-8").read()
        for m in re.finditer(r"\(0x([0-9A-Fa-f]{4})u16, 0x([0-9A-Fa-f]{4,6})u32\)", txt):
            sjis[int(m.group(1), 16)] = int(m.group(2), 16)
    cn = {}
    path = os.path.join(ROOT, "src", "codec", "cn_text.rs")
    if os.path.exists(path):
        for line in open(path, encoding="utf-8"):
            m = re.match(r"\s*\(0x([0-9A-Fa-f]+)[^,]*,?\s*0x([0-9A-Fa-f]+)", line)
            if m:
                cn[int(m.group(1), 16)] = int(m.group(2), 16)
    chars = set()
    script_dir = os.path.join(ROOT, "data", "script_cn")
    if not os.path.isdir(script_dir):
        return None
    for name in os.listdir(script_dir):
        if not name.endswith(".txt"):
            continue
        data = open(os.path.join(script_dir, name), "rb").read()
        i = 0
        while i < len(data):
            b = data[i]
            if 0x81 <= b <= 0x9F or 0xE0 <= b <= 0xFC:
                if i + 1 >= len(data):
                    break
                code = (b << 8) | data[i + 1]
                cp = cn.get(code) or sjis.get(code)
                if cp:
                    chars.add(chr(cp))
                i += 2
            else:
                if 0x20 <= b < 0x7F:
                    chars.add(chr(b))
                i += 1
    return chars


def coverage(path, face, corpus):
    if corpus is None:
        return None
    try:
        from fontTools.ttLib import TTFont
    except ImportError:
        return None
    f = TTFont(path, fontNumber=face, lazy=True)
    cmap = set(f.getBestCmap().keys())
    have = sum(1 for ch in corpus if ord(ch) in cmap)
    return 100.0 * have / max(len(corpus), 1)


def family_of(path, face):
    try:
        from fontTools.ttLib import TTFont
    except ImportError:
        return ""
    f = TTFont(path, fontNumber=face, lazy=True)
    names = {}
    for nr in f["name"].names:
        if nr.platformID == 3:
            names.setdefault(nr.nameID, nr.toUnicode())
    # Prefer the typographic family (16) because that is what FreeType reports
    # for the Medium face; fall back to the legacy family (1), stripping a
    # trailing weight word.
    return names.get(16) or re.sub(r"\s+(Medium|Regular|Light|Bold)$", "", names.get(1, ""))


def find_source(name, from_dir=None):
    dirs = [from_dir] if from_dir else []
    dirs += SYSTEM_FONT_DIRS
    for d in dirs:
        if d and os.path.isfile(os.path.join(d, name)):
            return os.path.join(d, name)
    return None


def find_license():
    for path in LICENSE_SOURCES:
        if os.path.isfile(path):
            return path
    return None


def cmd_build(from_dir):
    os.makedirs(LICENSE_DIR, exist_ok=True)
    corpus = script_corpus()
    if corpus is None:
        print("warning: data/script_cn not found; coverage will be recorded as n/a")
    rows = []
    for name, face, expected in PACK:
        src = find_source(name, from_dir)
        if not src:
            print(f"missing {name}: pass --from <dir> or install the Noto CJK package")
            return 1
        dst = os.path.join(FONT_DIR, name)
        if os.path.abspath(src) != os.path.abspath(dst):
            shutil.copyfile(src, dst)
        fam = family_of(dst, face)
        if fam != expected:
            print(f"warning: {name} face {face} family is {fam!r}, expected {expected!r}")
        cov = coverage(dst, face, corpus)
        rows.append((name, fam, face, cov, os.path.getsize(dst), sha256_file(dst)))
        print(f"copied {name} ({rows[-1][4]} bytes, family={fam}, coverage={cov if cov is None else round(cov, 1)}%)")
    lic = find_license()
    if lic:
        dst = os.path.join(LICENSE_DIR, "NotoSansCJK-OFL.txt")
        shutil.copyfile(lic, dst)
        print(f"copied licence {lic} -> {dst}")
    else:
        print("warning: no Noto CJK licence found; put it in fonts/LICENSES/ manually")
    with open(MANIFEST, "w", encoding="utf-8") as f:
        f.write(f"# wa2 font pack version {PACK_VERSION}\n")
        f.write("# path\tfamily\tface\tcoverage\tsize\tsha256\n")
        for name, fam, face, cov, size, digest in rows:
            cov_s = "n/a" if cov is None else f"{cov:.1f}"
            f.write(f"{name}\t{fam}\t{face}\t{cov_s}\t{size}\t{digest}\n")
    print(f"wrote {MANIFEST}")
    return 0


def cmd_check():
    if not os.path.isfile(MANIFEST):
        print(f"no manifest at {MANIFEST}; run without --check first")
        return 1
    bad = 0
    for line in open(MANIFEST, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) < 6:
            print(f"bad manifest row: {line!r}")
            bad += 1
            continue
        name, fam, face, _cov, size, digest = parts[:6]
        path = os.path.join(FONT_DIR, name)
        if not os.path.isfile(path):
            print(f"MISSING {name}")
            bad += 1
            continue
        real_size = os.path.getsize(path)
        real_digest = sha256_file(path)
        if str(real_size) != size or real_digest != digest:
            print(f"CHANGED {name}: size {size}->{real_size}, sha256 {digest}->{real_digest}")
            bad += 1
        else:
            print(f"ok {name} face={face} family={fam} size={size}")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="verify the manifest against fonts/")
    ap.add_argument("--from", dest="from_dir", default=None, help="directory holding the .ttc files")
    args = ap.parse_args()
    if args.check:
        return cmd_check()
    return cmd_build(args.from_dir)


if __name__ == "__main__":
    sys.exit(main())
