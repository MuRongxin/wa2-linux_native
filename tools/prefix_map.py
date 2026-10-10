import struct, glob, os
from collections import defaultdict, Counter

path = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak'
f = open(path, 'rb')
n = struct.unpack_from('<I', f.read(16), 12)[0]
nameset = set()
prefixes = set()
for i in range(n):
    f.seek(16 + i*44)
    e = f.read(44)
    nm = e[4:28].split(b'\x00')[0].decode('shift-jis', 'replace')
    nameset.add(nm)
    prefixes.add(nm[:3])
by_lower = {nm.lower(): nm for nm in nameset}

# collect all (cid, series, variant) triples
triples = set()
for bp in sorted(glob.glob('/home/lrxin/wa2tool/extract/script/*.bnr')):
    bnr = open(bp, 'rb').read()
    nn = len(bnr)
    def w(off): return struct.unpack_from('<I', bnr, off)[0]
    count = w(8); entry = 28
    for k in range(count):
        b = 12 + k*8
        if w(b) == 0:
            entry = w(b+4); break
    i = entry
    recs = []
    cur = None
    while i + 12 <= nn:
        op = w(i)
        if op == 6:
            cur = (w(i+4), []); recs.append(cur); i += 8
        elif op == 5:
            if cur: cur[1].append(w(i+8))
            i += 12
        elif op in (3, 4):
            if op == 4 and cur: cur[1].append(-1)
            i += 8
        elif op == 0:
            sub = w(i+4)
            i += 12 if sub == 12 else (16 if sub in (1,2) else 8)
        elif op in (1, 2):
            i += 8
        else:
            i += 4
    for idx in range(len(recs) - 3):
        kind, vals = recs[idx]
        if kind != 30:
            continue
        if len(vals) == 3 and all(v >= 0 for v in vals):
            cid, variant, flag = vals
            if cid < 40 and flag % 1000 == 0 and 1000 <= flag <= 30000 and variant < 1000:
                triples.add((cid, flag // 1000, variant))

# for each cid, find best prefix
best = {}
for cid in sorted({t[0] for t in triples}):
    score = Counter()
    for (c2, series, variant) in triples:
        if c2 != cid:
            continue
        for pf in prefixes:
            fn = f'{pf}{series:03d}{variant:03d}.tga'
            if fn in nameset or fn.lower() in by_lower:
                score[pf] += 1
    total = sum(1 for t in triples if t[0] == cid)
    if score:
        pf, cnt = score.most_common(1)[0]
        best[cid] = (pf, cnt, total)
        print(f'char {cid}: prefix={pf}  {cnt}/{total} matched  (candidates: {score.most_common(3)})')
    else:
        print(f'char {cid}: NO MATCH ({total} triples)')

print()
print('final mapping:', {k: v[0] for k, v in sorted(best.items())})