import struct, glob
from collections import Counter

tri = Counter()
per_char_tri = {}
for bp in sorted(glob.glob('/home/lrxin/wa2tool/extract/script/*.bnr')):
    bnr = open(bp, 'rb').read()
    n = len(bnr)
    def w(off): return struct.unpack_from('<I', bnr, off)[0]
    count = w(8)
    entry = 28
    for k in range(count):
        b = 12 + k*8
        if w(b) == 0:
            entry = w(b+4); break
    i = entry
    vals = []
    while i + 12 <= n:
        op = w(i)
        if op == 6:
            vals = []; i += 8
        elif op == 5:
            vals.append(w(i+8)); i += 12
        elif op in (3, 4):
            if op == 4 and w(i+4) == 138 and len(vals) >= 5:
                cid = vals[-5]
                t = (vals[-4], vals[-3], vals[-2])
                tri[t] += 1
                per_char_tri.setdefault(cid, Counter())[t] += 1
            i += 8
        elif op == 0:
            sub = w(i+4)
            i += 12 if sub == 12 else (16 if sub in (1,2) else 8)
        elif op in (1, 2):
            i += 8
        else:
            i += 4

print('=== distinct (p1,p2,p3) triples of ACT138 (game-wide) ===')
for t, c in tri.most_common(20):
    print(f'  {t}: {c}')
print()
print('=== per character top triples ===')
for cid in sorted(per_char_tri)[:12]:
    top = per_char_tri[cid].most_common(3)
    print(f'  char {cid}: {top}')