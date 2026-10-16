#!/usr/bin/env python3
"""Generate the authoritative char -> glyph-atlas cell map for WA2.

The game exe ships a font table: an array of u16 Shift-JIS *code ranges*
(`code1, code2` pairs) listing every code that has a glyph, in glyph-cell
order.  Expanding those ranges therefore yields the cell -> char list directly,
with no guessing and no hand-tuned formulas.

Verified empirically (2026-10-10):

    atlas_cell = 139 + index_in_table

  * WA2.exe      (Japanese) table @ 656064 (0xA02C0) + fnt.pak/本体80.tga
      "とうとう、降ってきた。" renders exactly right
  * WA2_chs.exe  (CK-GAL Chinese patch) table @ 656576 (0xA0420) + fon.pak/本体80.tga
      "终于，还是下起来了。" renders exactly right

The character payload of the Chinese patch is Shift-JIS-coded (it repaints
Japanese kanji cells with the Chinese glyph); decode ck-gal.pak .txt as SJIS and
render with fon.pak's atlas + the WA2_chs.exe table.

Usage:
    python3 tools/gen_cell_map.py                      # both, default paths
    python3 tools/gen_cell_map.py --exe <exe> --base <off> --out <file>

Output format (TSV):  <sjis-code-hex>  <char>  <atlas-cell>
"""

import argparse
import os
import struct
import sys

DEFAULT_GAME = "/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版"
# ASCII lives at cell (code - 0x20); the double-byte block starts at cell 139.
CELL_OFFSET = 139


def find_table_bases(exe_bytes):
    """Locate the font table(s) by their first range pair (0x8140, 0x81AC)."""
    needle = struct.pack("<HH", 0x8140, 0x81AC)
    hits, i = [], 0
    while True:
        i = exe_bytes.find(needle, i)
        if i < 0:
            break
        hits.append(i)
        i += 1
    return hits


def expand_table(exe_bytes, base):
    """Expand the u16 range pairs into the cell-ordered code list.

    Returns [(sjis_code, char_or_None), ...]; index in the list == table index.
    """
    out = []
    j = 0
    while j < 6000:
        p = base + j * 2
        if p + 3 >= len(exe_bytes):
            break
        c1 = exe_bytes[p] | (exe_bytes[p + 1] << 8)
        c2 = exe_bytes[p + 2] | (exe_bytes[p + 3] << 8)
        if c1 == 0 and c2 == 0:
            break
        if c2 > c1 and (c2 >> 8) >= (c1 >> 8):
            # A range: for each lead byte, walk the valid trail bytes.
            for hi in range(c1 >> 8, (c2 >> 8) + 1):
                lo_s = (c1 & 0xFF) if hi == (c1 >> 8) else 0x40
                lo_e = (c2 & 0xFF) if hi == (c2 >> 8) else 0xFC
                for lo in range(lo_s, lo_e + 1):
                    # NOTE: do NOT skip 0x7F even though it is not a valid
                    # Shift-JIS trail byte - the atlas still reserves a cell for
                    # it.  Skipping it shifts every later glyph by 1 (verified:
                    # ム would land on cell 505 instead of 507).
                    out.append((hi << 8 | lo, decode_sjis(hi << 8 | lo)))
            j += 2
        else:
            out.append((c1, decode_sjis(c1)))
            j += 1
    return out


def decode_sjis(code):
    try:
        return bytes([code >> 8, code & 0xFF]).decode("shift-jis")
    except Exception:
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", default=DEFAULT_GAME)
    ap.add_argument("--exe", default=None)
    ap.add_argument("--base", type=int, default=None)
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    if args.exe is None:
        # Emit both maps.
        jobs = [
            ("WA2.exe", os.path.join("tools", "tables", "wa2_cell_map_jp.tsv")),
            ("WA2_chs.exe", os.path.join("tools", "tables", "wa2_cell_map_cn.tsv")),
        ]
    else:
        jobs = [(os.path.basename(args.exe), args.out or "cell_map.tsv")]

    for exe_name, out_path in jobs:
        exe_path = args.exe or os.path.join(args.game, exe_name)
        data = open(exe_path, "rb").read()
        base = args.base
        if base is None:
            hits = find_table_bases(data)
            if not hits:
                print(f"{exe_name}: font table not found", file=sys.stderr)
                continue
            base = hits[-1]  # the longer table (the one that has ASCII/kana)
        table = expand_table(data, base)
        lines = [
            "# sjis_code\tchar\tatlas_cell  (cell = 139 + table index)",
            f"# source: {os.path.basename(exe_path)} table @ {base} (0x{base:X})",
        ]
        # Every table entry is emitted, including codes that are not valid
        # Shift-JIS (trail byte 0x7F): the atlas still reserves a cell for
        # them, and dropping the row left those glyphs unresolvable. The char
        # column stays empty when the code has no Shift-JIS decoding.
        mapped = 0
        for i, (code, ch) in enumerate(table):
            lines.append(f"0x{code:04X}\t{ch or ''}\t{CELL_OFFSET + i}")
            mapped += 1
        os.makedirs(os.path.dirname(out_path), exist_ok=True)
        with open(out_path, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
        print(f"{exe_name}: table @ 0x{base:X}, {len(table)} entries, "
              f"{mapped} mapped -> {out_path}")


if __name__ == "__main__":
    main()
