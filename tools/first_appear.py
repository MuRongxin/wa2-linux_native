import struct, glob, os
from collections import Counter, defaultdict

first_ctx = defaultdict(list)   # (char) -> list of (scene, prev_actions)
act_ids = Counter()

for bp in sorted(glob.glob('/home/lrxin/wa2tool/extract/script/*.bnr'))[:60]:
    scene = os.path.basename(bp)[:-4]
    bnr = open(bp, 'rb').read()
    n = len(bnr)
    def w(off): return struct.unpack_from('<I', bnr, off)[0]
    count = w(8); entry = 28
    for k in range(count):
        b = 12 + k*8
        if k < count and w(b) == 0:
            entry = w(b+4); break
    i = entry
    recs = []          # list of (kind, ops list) where ops = [('act',id),('int',v)...]
    cur = None
    while i + 12 <= n:
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

    seen = set()
    for idx, (kind, ops) in enumerate(recs):
        acts = [v for k, v in ops if k == 'a']
        if 138 in acts:
            ints = [v for k, v in ops if k == 'i']
            if len(ints) >= 5:
                cid = ints[-5]
                if cid not in seen and cid < 40:
                    seen.add(cid)
                    prevs = []
                    for j in range(max(0, idx-4), idx):
                        pk, pops = recs[j]
                        pa = [v for k, v in pops if k == 'a']
                        pi = [v for k, v in pops if k == 'i']
                        prevs.append(f'K{pk}:acts={pa},ints={pi[:6]}')
                    first_ctx[cid].append((scene, prevs))
                    for a in acts:
                        act_ids[a] += 1

print('=== first-appearance context per char (first 6 samples) ===')
for cid in sorted(first_ctx)[:14]:
    print(f'--- char {cid} ---')
    for scene, prevs in first_ctx[cid][:3]:
        print(f'  scene {scene}:')
        for p in prevs[-3:]:
            print(f'     {p}')
print()
print('=== actions seen in first-appearance records ===', act_ids.most_common(12))