import struct, glob
from collections import defaultdict

path = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak'
f = open(path, 'rb')
n = struct.unpack_from('<I', f.read(16), 12)[0]
files = []
for i in range(n):
    f.seek(16 + i*44)
    e = f.read(44)
    name = e[4:28].split(b'\x00')[0].decode('shift-jis', 'replace')
    files.append(name)

prefix = {0:'har',1:'kaz',2:'set',3:'koh',4:'izu',5:'mar',10:'tak',11:'ioo',15:'?',16:'you',
          19:'tom',20:'sat',23:'say',24:'aco',25:'mih',27:'ueh',28:'kiz',30:'ham',31:'mat',
          33:'suz',34:'saw',35:'miy'}

# samples collected earlier (char_id, p4)
samples = {
    0: [30, 32, 34, 36, 38],
    1: [113, 115, 117, 323, 326],
    2: [120, 122, 124, 126, 128],
    3: [72, 75, 78, 81, 120],
    4: [199, 204, 207, 211, 213],
    5: [175, 184, 186, 189, 191],
    10: [0, 2, 4, 6, 8],
    11: [278, 280, 282, 284, 287],
    20: [5, 7, 9, 11, 13],
    16: [486, 487, 488, 489, 490],
}

# build per-prefix variant sets: filename = ppp SSS VVV . tga  (SSS = series)
def variants(pfx):
    out = set()
    for nm in files:
        if nm.startswith(pfx) and len(nm) >= 12:
            core = nm[:-4]
            try:
                num = int(core[3:])
            except ValueError:
                continue
            out.add(num)  # SSSVVV as integer
    return out

print('=== test: does (prefix, p4) match a variant? ===')
for cid, ps in samples.items():
    pf = prefix.get(cid, '?')
    vs = variants(pf)
    if not vs:
        print(f'char {cid} ({pf}): no files')
        continue
    hit = 0
    detail = []
    for p in ps:
        # try: variant == p  (SSSVVV == p)
        m1 = p in vs
        # try: variant ends with p as 3-digit VVV (any series)
        m2 = any(str(v).endswith(f'{p:03d}') for v in vs)
        detail.append(f'{p}:exact={m1},suffix={m2}')
        if m1 or m2: hit += 1
    print(f'char {cid:>2} ({pf}, {len(vs)} variants): {hit}/{len(ps)}  ' + ' | '.join(detail))

# also show variant ranges for a couple of chars
for cid in (1, 2, 4, 11):
    pf = prefix[cid]
    vs = sorted(variants(pf))
    print(f'{pf}: min={vs[0]} max={vs[-1]} n={len(vs)} first20={vs[:20]} last10={vs[-10:]}')