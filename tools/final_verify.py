import struct, glob, os
from collections import Counter, defaultdict

path = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak'
f = open(path, 'rb')
n = struct.unpack_from('<I', f.read(16), 12)[0]
nameset = set()
for i in range(n):
    f.seek(16 + i*44)
    e = f.read(44)
    nameset.add(e[4:28].split(b'\x00')[0].decode('shift-jis', 'replace'))
by_lower = {nm.lower(): nm for nm in nameset}

# semantic-first candidate lists (from auto analysis + romaji)
cands = {
    0: ['har'], 1: ['kaz'], 2: ['set'], 3: ['koh'], 4: ['izu'], 5: ['mar'],
    10: ['tak', 'oto'], 11: ['ioo', 'tak'], 12: ['oto'], 13: ['pap'], 14: ['oto', 'mam'],
    15: ['tak', 'oto'], 16: ['you'], 17: ['oto'], 18: ['kaz'], 19: ['tom'], 20: ['Sat', 'sat'],
    21: ['Sat'], 22: ['nak'], 23: ['say'], 24: ['aco', 'koh'], 25: ['mih'], 27: ['ueh', 'mat'],
    28: ['kiz', 'yos'], 29: ['koh'], 30: ['ham', 'mat'], 31: ['mat'], 32: ['koh'], 33: ['suz', 'izu'],
    34: ['saw'], 35: ['miy', 'tom'], 36: ['koh'], 37: ['koh'],
}

hits = 0; misses = 0; miss_samples = []
per_c = Counter(); per_hit = Counter()
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
            if cur: cur[1].append(('i', w(i+8)))
            i += 12
        elif op == 3:
            if cur: cur[1].append(('t', w(i+4)))
            i += 8
        elif op == 4:
            if cur: cur[1].append(('a', w(i+4)))
            i += 8
        elif op == 0:
            sub = w(i+4)
            i += 12 if sub == 12 else (16 if sub in (1,2) else 8)
        elif op in (1, 2):
            i += 8
        else:
            i += 4
    for idx in range(len(recs) - 3):
        kind, ops = recs[idx]
        ints = [v for k2, v in ops if k2 == 'i']
        others = [k2 for k2, v in ops if k2 != 'i']
        if kind == 30 and not others and len(ints) == 3:
            cid, variant, flag = ints
            if not (cid < 40 and flag % 1000 == 0 and 1000 <= flag <= 30000 and variant < 1000):
                continue
            # require following K16 with ACT154/155 within 3 records
            ok = False
            for j in range(idx+1, min(idx+4, len(recs))):
                k3, ops3 = recs[j]
                a3 = [v for k2, v in ops3 if k2 == 'a']
                if k3 == 16 and any(x in (154, 155) for x in a3):
                    ok = True; break
            if not ok:
                continue
            series = flag // 1000
            found = False
            for pf in cands.get(cid, []):
                fn = f'{pf}{series:03d}{variant:03d}.tga'
                if fn in nameset or fn.lower() in by_lower:
                    found = True; break
            per_c[cid] += 1
            if found:
                hits += 1; per_hit[cid] += 1
            else:
                misses += 1
                if len(miss_samples) < 20:
                    miss_samples.append((cid, series, variant, flag))

print(f'WITH lookahead: hits={hits} misses={misses}  acc={round(100*hits/(hits+misses),1)}%')
print('miss samples:', miss_samples)
print()
print('per char hit rates:')
for cid in sorted(per_c):
    print(f'  char {cid:>2}: {per_hit[cid]}/{per_c[cid]}')