import struct, glob, os, re
from collections import defaultdict, Counter

path = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak'
f = open(path, 'rb')
n = struct.unpack_from('<I', f.read(16), 12)[0]
nameset = set()
for i in range(n):
    f.seek(16 + i*44)
    e = f.read(44)
    nameset.add(e[4:28].split(b'\x00')[0].decode('shift-jis', 'replace'))
by_lower = {nm.lower(): nm for nm in nameset}

prefix_of = {0:'har',1:'kaz',2:'set',3:'koh',4:'izu',5:'mar',10:'tak',11:'ioo',15:'tak',
             16:'you',19:'tom',20:'sat',23:'say',24:'aco',25:'mih',27:'ueh',28:'kiz',
             30:'ham',31:'mat',33:'suz',34:'saw',35:'miy'}

hits, misses = [], []
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
        acts = [v for k2, v in ops if k2 == 'a']
        if not acts and len(ints) == 3:
            cid, variant, flag = ints
            if cid < 40 and flag % 1000 == 0 and 1000 <= flag <= 30000 and variant < 1000:
                series = flag // 1000
                pf = prefix_of.get(cid)
                if not pf:
                    continue
                fname = f'{pf}{series:03d}{variant:03d}.tga'
                if fname in nameset:
                    hits.append(fname)
                elif fname.lower() in by_lower:
                    hits.append(by_lower[fname.lower()])
                else:
                    misses.append((cid, pf, series, variant, fname))

print(f'hits: {len(hits)}, misses: {len(misses)}  ({round(100*len(hits)/max(1,len(hits)+len(misses)),1)}% hit)')
print('top hits:', Counter(hits).most_common(8))
print('miss samples:', misses[:15])
print('misses by char:', Counter((m[0], m[1]) for m in misses).most_common(10))