import struct

def load(name):
    bnr = open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()
    txt = open(f'/home/lrxin/wa2tool/extract/script/{name}.txt', 'rb').read()
    return bnr, txt.split(b',')

bnr, tokens = load('2003')
n = len(bnr)
words = [struct.unpack_from('<I', bnr, i)[0] for i in range(28, n - 3, 4)]
base = 28

# find all big values at any word position
bigs = []
for k, v in enumerate(words):
    if 64 <= v < n:
        bigs.append((base + k*4, v))
print(f'total big values (64..{n}): {len(bigs)}')

# print contexts
print('\n--- contexts of big values (first 40) ---')
for off, v in bigs[:40]:
    k = (off - base) // 4
    ctx_before = words[max(0,k-4):k]
    ctx_after = words[k+1:k+4]
    print(f'off={off:6d} val={v:6d}  before={ctx_before}  after={ctx_after}')

# do these values point near record starts? check byte at target-? 
print('\n--- target alignment check ---')
aligned = 0
for off, v in bigs:
    # try: is there a 6-word within +-8 bytes?
    ok = False
    for delta in range(-8, 9, 4):
        p = v + delta
        if 28 <= p < n - 4:
            w = struct.unpack_from('<I', bnr, p)[0]
            if w == 6:
                ok = True
                break
    if ok: aligned += 1
print(f'{aligned}/{len(bigs)} targets have a 6 within +-8 bytes')

# how many big values, distribution
vals = sorted(set(v for _, v in bigs))
print(f'\ndistinct target values: {len(vals)}')
print(vals[:40])