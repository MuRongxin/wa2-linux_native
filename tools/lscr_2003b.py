import struct

def load(name):
    bnr = open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()
    txt = open(f'/home/lrxin/wa2tool/extract/script/{name}.txt', 'rb').read()
    return bnr, txt.split(b',')

def decode(bnr):
    i = 28; n = len(bnr); records = []; cur = None
    while i + 4 <= n:
        op = struct.unpack_from('<I', bnr, i)[0]
        if op == 6:
            cur = (i, struct.unpack_from('<I', bnr, i+4)[0], [])
            records.append(cur); i += 8
        elif op in (0, 1, 2, 3, 4):
            a = struct.unpack_from('<I', bnr, i+4)[0]
            if cur: cur[2].append((f'op{op}', a))
            i += 8
        elif op == 5:
            t = struct.unpack_from('<I', bnr, i+4)[0]
            v = struct.unpack_from('<I', bnr, i+8)[0]
            kind = 'i' if t == 3 else ('f' if t == 4 else f't{t}')
            val = v if t == 3 else struct.unpack('<f', struct.pack('<I', v))[0]
            if cur: cur[2].append((kind, val))
            i += 12
        else:
            if cur: cur[2].append((f'OPER{op}', None))
            i += 4
    return records

bnr, tokens = load('2003')
recs = decode(bnr)
print(f'records: {len(recs)}')

def show(recs, lo=-1, hi=-1, only_off=None):
    for idx, (off, kind, vals) in enumerate(recs):
        if only_off is not None and off != only_off: continue
        if lo >= 0 and not (lo <= idx <= hi): continue
        ms = []
        for k, v in vals:
            if k == 'op3':
                s = tokens[v].decode('shift-jis','replace')[:30] if v < len(tokens) else 'oob'
                ms.append(f'tok#{v}({s!r})')
            elif k == 'f': ms.append(f'f={v:.1f}')
            else: ms.append(f'{k}={v}')
        print(f'[{idx:4d}] {off:6d} K{kind:3d}: ' + ', '.join(ms))

# find records referencing tok 244..250
print('=== records referencing tok#244..250 ===')
target = set(range(244, 251))
hits = [i for i, (off, k, vals) in enumerate(recs) if any(kk == 'op3' and vv in target for kk, vv in vals)]
for i in hits:
    show(recs, only_off=recs[i][0])
print(f'hit record indices: {hits}')

# dump context around the first hit group
if hits:
    lo = max(0, hits[0]-4); hi = min(len(recs)-1, hits[-1]+14)
    print(f'\n=== context [{lo}..{hi}] ===')
    show(recs, lo, hi)