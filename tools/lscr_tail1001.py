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

bnr, tokens = load('1001')
print(f'1001 tokens: {len(tokens)}')
print('--- last 12 tokens ---')
for j in range(len(tokens)-12, len(tokens)):
    print(f'  tok#{j}: {tokens[j].decode("shift-jis","replace")!r}')

recs = decode(bnr)
print(f'\n--- last 16 records of 1001 ---')
for off, kind, vals in recs[-16:]:
    ms = []
    for k, v in vals:
        if k == 'op3':
            ms.append(f'tok#{v}({tokens[v].decode("shift-jis","replace")[:26]!r})' if v < len(tokens) else f'tok#{v}(oob)')
        elif k == 'f': ms.append(f'f={v:.1f}')
        else: ms.append(f'{k}={v}')
    print(f'{off:5d}  KIND {kind:3d}: ' + ', '.join(ms))