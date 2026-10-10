import struct

def load(name):
    bnr = open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()
    txt = open(f'/home/lrxin/wa2tool/extract/script/{name}.txt', 'rb').read()
    return bnr, txt.split(b',')

bnr, tokens = load('2003')

# global walk collecting op3 references and raw values
i = 28; n = len(bnr)
op3s = []      # (off, tok)
allops = []
while i + 4 <= n:
    op = struct.unpack_from('<I', bnr, i)[0]
    if op == 5:
        i += 12
    elif op <= 64:
        a = struct.unpack_from('<I', bnr, i+4)[0] if i+8 <= n else None
        if op == 3: op3s.append((i, a))
        allops.append((i, op, a))
        i += 8
    else:
        i += 4

def first_toks_after(off, k=6):
    out = []
    for o, t in op3s:
        if o > off:
            s = tokens[t].decode('shift-jis', 'replace')[:36] if t < len(tokens) else 'oob'
            out.append((o, t, s))
            if len(out) >= k: break
    return out

for tgt, label in [(36772, 'A(option1?)'), (39696, 'B(option2?)'), (34132, 'fall-through')]:
    print(f'=== target {label} @ {tgt} ===')
    print(f'  first ops at target: ', end='')
    j = tgt
    for _ in range(6):
        v = struct.unpack_from('<I', bnr, j)[0]
        print(f'{v}@{j}', end=' ')
        j += 4
    print()
    for o, t, s in first_toks_after(tgt, 5):
        print(f'   next tok: off={o} tok#{t} {s!r}')
    print()

# The second choice at tok#988/989 - find its branch record
print('=== second choice tok#988/989 ===')
target = set([988, 989, 990, 991])
for o, op, a in allops:
    if op == 3 and a in target:
        print(f'  off={o} op3 tok#{a}')
# find nearby branch (offset values > 64 in stream)
print()
print('--- raw scan 1e5..1e5+? around choice2 ---')
for o, op, a in allops:
    if op == 3 and a == 988:
        start = o
        # print 200 bytes from start
        for k in range(start, start+160, 4):
            v = struct.unpack_from('<I', bnr, k)[0]
            print(f'  {k}: {v}', end='')
            print()
        break