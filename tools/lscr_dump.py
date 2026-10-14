#!/usr/bin/env python3
"""Dump LSCR ops with record structure for a scene, mirroring the Rust decoder.

Usage: python3 lscr_dump.py <scene_id> [max_steps]
See docs/LSCR舞台指令逆向报告.md for the opcode semantics.

Record kinds: K30 normal / K16 sprite-apply / K23 structural / K18 ...
Val op 5: type 3 = int, type 4 = float.
Tok op 3: index into the comma-separated .txt string pool.
Act op 4: stage/page action (131 text, 132 wait, 144 speaker, 138 voice/focus,
          146/147 background, 154/155 sprite apply, 165 sound, 176 graphic slot,
          194 frame-timed wait, 208 choice option, 193 movie, 172 tint).
"""
import struct, sys

scene = sys.argv[1] if len(sys.argv) > 1 else "1001"
base = "/home/lrxin/wa2-native/data/script"
bnr = open(f"{base}/{scene}.bnr", "rb").read()
txt = open(f"{base}/{scene}.txt", "rb").read()

# decode the fake-SJIS text as cp932 with replacement (JP script here)
tokens = txt.split(b",")
def tok_str(n):
    if 0 <= n < len(tokens):
        s = tokens[n].decode("cp932", "replace").replace("\n", "\\n")
        return s[:52]
    return "<oob>"

magic, hsize, lcount = struct.unpack_from("<III", bnr, 0)
labels = []
for k in range(lcount):
    lid, off = struct.unpack_from("<II", bnr, 12 + k*8)
    labels.append((lid, off))
entry = next((o for i, o in labels if i == 0), 28)
print(f"LSCR {scene}: magic={magic} hsize={hsize} labels={labels[:8]} entry={entry:#x} tokens={len(tokens)}")
print("=" * 100)

ACT = {131:"TEXT", 132:"WAIT", 144:"SPEAKER", 138:"VOICE/FOCUS", 172:"TINT",
       193:"MOVIE", 154:"SPR-APPLY", 155:"SPR-APPLY2", 176:"LOAD-GRAPHIC",
       208:"CHOICE-OPT", 0:"JUMP", 1:"CATCH"}

i = entry; n = len(bnr); step = 0
while i + 4 <= n and step < int(sys.argv[2] if len(sys.argv) > 2 else 500):
    off = i
    op = struct.unpack_from("<I", bnr, i)[0]
    if op == 6:
        kind = struct.unpack_from("<I", bnr, i+4)[0]
        print(f"\n--- Rec@{off:5d} kind={kind}")
        i += 8
    elif op == 5:
        t, v = struct.unpack_from("<II", bnr, i+4)
        if t == 4:
            f = struct.unpack("<f", struct.pack("<I", v))[0]
            print(f"{off:5d}: INT/FLT type={t} val={f}")
        else:
            print(f"{off:5d}: INT/FLT type={t} val={v}")
        i += 12
    elif op == 3:
        a = struct.unpack_from("<I", bnr, i+4)[0]
        print(f"{off:5d}: TOK[{a:4d}] = {tok_str(a)!r}")
        i += 8
    elif op == 4:
        a = struct.unpack_from("<I", bnr, i+4)[0]
        print(f"{off:5d}: ACT {a} ({ACT.get(a,'?')})")
        i += 8
    elif op == 0:
        sub = struct.unpack_from("<I", bnr, i+4)[0]
        if sub == 12:
            t = struct.unpack_from("<I", bnr, i+8)[0]
            print(f"{off:5d}: GOTO {t:#x}")
            i += 12
        elif sub == 2:
            a, b_ = struct.unpack_from("<II", bnr, i+8)
            print(f"{off:5d}: BRANCH opt1->fallthru opt2->{a:#x} merge={b_:#x}")
            i += 16
        else:
            print(f"{off:5d}: OP0 sub={sub}")
            i += 8
    elif op in (1, 2):
        sub = struct.unpack_from("<I", bnr, i+4)[0]
        print(f"{off:5d}: {'VAR' if op==1 else 'FUN'} sub={sub}")
        i += 8
    else:
        print(f"{off:5d}: RAW {op}")
        i += 4
    step += 1
