#!/usr/bin/env python3
"""Which event CGs a scenario loads, read straight out of the bytecode.

ACT148 loads a picture as v{id*10+variant:06}; ACT201 turns a layer of one on
as v{id*10+layer}. Walking the ops the way the interpreter does (a record is
Rec, then its ints, then the action) gives the file names without guessing.

The H-scene list is what data/script_cn/9999.bnr replays - its token pool is
literally the scenario ids of the サンプルプレイ entries, and ACT221/ACT222 (the
only two opcodes that appear in that same set of scenarios and nowhere else)
are the calls that pick one.
"""

from __future__ import annotations

import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "data"

#: Scenarios the sample-play / recall mode can jump to.
H_SCENES = [
    "1012_030_2", "2031", "2312", "2313", "2401", "2402", "2504", "2507",
    "2511", "2513", "3001", "3015", "3210", "3901", "3902", "3904", "3906", "3907",
]

_ACTOP = bytes((4, 0, 0, 0))
_INTOP = bytes((5, 0, 0, 0, 3, 0, 0, 0))
_RECOP = bytes((6, 0, 0, 0))


def _act(act: int) -> bytes:
    return _ACTOP + struct.pack("<I", act)


def _ints_before(data: bytes, end: int) -> list[int]:
    start = data.rfind(_RECOP, 0, end)
    start = start if start >= 0 else 0
    values: list[int] = []
    pos = start
    while pos + 12 <= end:
        if data[pos : pos + 8] == _INTOP:
            values.append(struct.unpack_from("<i", data, pos + 8)[0])
            pos += 12
        else:
            pos += 4
    return values


def scene_cgs(scenario: str, tree: str = "script_cn") -> list[str]:
    """Distinct v* CG file names the scenario asks for, in program order."""
    path = ROOT / tree / (scenario + ".bnr")
    if not path.is_file():
        return []
    data = path.read_bytes()
    found: list[str] = []
    for act in (148, 201):
        mark = _act(act)
        pos = 0
        while True:
            pos = data.find(mark, pos)
            if pos < 0:
                break
            end = pos + 8
            pos = end
            values = _ints_before(data, end)
            name = None
            if act == 148 and len(values) >= 2 and values[1] > 0:
                name = "v%06d" % (values[1] * 10 + max(values[2], 0))
            elif act == 201 and len(values) >= 3 and values[0] > 0:
                name = "v%06d" % (values[0] * 10 + max(values[1], 0))
            if name and name not in found:
                found.append(name)
    return found


def h_scene_cgs() -> list[str]:
    out: list[str] = []
    for scene in H_SCENES:
        for name in scene_cgs(scene):
            if name not in out:
                out.append(name)
    return out


if __name__ == "__main__":
    import sys

    if len(sys.argv) > 1 and sys.argv[1] == "--h":
        for scene in H_SCENES:
            cgs = scene_cgs(scene)
            print("%-12s %2d  %s" % (scene, len(cgs), " ".join(sorted(cgs))))
    else:
        for name in h_scene_cgs():
            print(name)
