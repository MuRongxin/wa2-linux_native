import struct

bnr = open('/home/lrxin/wa2tool/extract/script/1001.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/1001.txt', 'rb').read()
tokens = txt.split(b',')

def tok_str(n):
    if 0 <= n < len(tokens):
        return tokens[n].decode('shift-jis', 'replace').replace('\n', '\\n')[:44]
    return '<oob>'

# Decode into records: 6,KIND followed by typed values
i = 28; n = len(bnr)
records = []
cur = None
while i + 4 <= n:
    op = struct.unpack_from('<I', bnr, i)[0]
    if op == 6:
        cur = (i, struct.unpack_from('<I', bnr, i+4)[0], [])
        records.append(cur)
        i += 8
    elif op == 3:
        a = struct.unpack_from('<I', bnr, i+4)[0]
        if cur: cur[2].append(('tok', a))
        i += 8
    elif op == 4:
        a = struct.unpack_from('<I', bnr, i+4)[0]
        if cur: cur[2].append(('id', a))
        i += 8
    elif op == 5:
        t = struct.unpack_from('<I', bnr, i+4)[0]
        v = struct.unpack_from('<I', bnr, i+8)[0]
        kind = 'i' if t == 3 else ('f' if t == 4 else f't{t}')
        val = v if t == 3 else struct.unpack('<f', struct.pack('<I', v))[0]
        if cur: cur[2].append((kind, val))
        i += 12
    else:
        a = struct.unpack_from('<I', bnr, i+4)[0] if i+8 <= n else 0
        if cur: cur[2].append((f'op{op}', a))
        i += 8

print(f'records: {len(records)}')
for off, kind, vals in records[:45]:
    ms = []
    for v in vals:
        if v[0] == 'tok':
            ms.append(f'tok#{v[1]}({tok_str(v[1])!r})')
        elif v[0] == 'f':
            ms.append(f'f={v[1]:.1f}')
        else:
            ms.append(f'{v[0]}={v[1]}')
    print(f'{off:5d}  KIND {kind:3d}: ' + ', '.join(ms))