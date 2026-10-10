import struct, glob, os, re
from collections import defaultdict, Counter

# char.pak variant sets per prefix
path = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak'
f = open(path, 'rb')
n = struct.unpack_from('<I', f.read(16), 12)[0]
prefix_variants = defaultdict(set)   # prefix -> set of (series, variant)
all_names = []
for i in range(n):
    f.seek(16 + i*44)
    e = f.read(44)
    name = e[4:28].split(b'\x00')[0].decode('shift-jis', 'replace')
    all_names.append(name)
    m = re.match(r'^([A-Za-z]{3})(\d)(\d{3})\.tga$', name)
    if m:
        prefix_variants[m.group(1).lower()].add((int(m.group(2)), int(m.group(3))))

prefix_of = {0:'har',1:'kaz',2:'set',3:'koh',4:'izu',5:'mar',10:'tak',11:'ioo',15:'tak',
             16:'you',19:'tom',20:'sat',23:'say',24:'aco',25:'mih',27:'ueh',28:'kiz',
             30:'ham',31:'mat',33:'suz',34:'saw',35:'miy'}

# find "sprite load" records: int triple (cid, variant, ~1000/18000) followed by K16 ACT154/155
pairs = Counter()
triple_samples = defaultdict(list)
for bp in sorted(glob.glob('/home/lrxin/wa2tool/extract/script/*.bnr')):
    scene = os.path.basename(bp)[:-4]
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
            if cid < 40 and 1000 <= flag <= 20000:
                # look ahead within 3 records for K16 with ACT154/155
                for j in range(idx+1, min(idx+4, len(recs))):
                    k3, ops3 = recs[j]
                    a3 = [v for k2, v in ops3 if k2 == 'a']
                    if any(x in (154, 155) and k3 == 16 for x in a3):
                        pairs[(cid, variant)] += 1
                        if len(triple_samples[cid]) < 10:
                            triple_samples[cid].append((scene, variant, flag))
                        break

print(f'collected {len(pairs)} distinct (cid, variant) pairs, total {sum(pairs.values())}')
# verify against char.pak: does a file exist with that variant (any series)?
hit, miss = 0, 0
miss_list = []
for (cid, variant), cnt in sorted(pairs.items(), key=lambda kv: -kv[1]):
    pf = prefix_of.get(cid)
    if not pf:
        continue
    vs = {v for (s, v) in prefix_variants.get(pf, set())}
    if variant in vs:
        hit += cnt
    else:
        miss += cnt
        if len(miss_list) < 25:
            miss_list.append((cid, pf, variant, cnt))
print(f'variant exists in char.pak: {hit} occurrences; missing: {miss}')
print('sample misses (cid, prefix, variant, count):')
for m in miss_list:
    print('  ', m)
print()
print('samples per char:')
for cid in sorted(triple_samples)[:12]:
    pf = prefix_of.get(cid, '?')
    print(f'  char {cid} ({pf}): {triple_samples[cid][:5]}')