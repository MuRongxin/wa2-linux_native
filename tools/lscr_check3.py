import struct

def u32s(data, start=0):
    return [struct.unpack_from('<I', data, i)[0] for i in range(start, len(data) - 3, 4)]

bnr = open('/home/lrxin/wa2tool/extract/script/1001.bnr', 'rb').read()

# hexdump tail
print('--- tail 160 bytes ---')
tail = bnr[-160:]
for i in range(0, len(tail), 16):
    chunk = tail[i:i+16]
    hexpart = ' '.join(f'{b:02x}' for b in chunk)
    asc = ''.join(chr(b) if 32 <= b < 127 else '.' for b in chunk)
    print(f'{len(bnr)-160+i:5d}  {hexpart:<48} {asc}')

# count "6, 30" (0x0000001E) occurrences as u32 pairs
vals = u32s(bnr, 28)
cnt_6_30 = 0
for i in range(len(vals)-1):
    if vals[i] == 6 and vals[i+1] == 30:
        cnt_6_30 += 1
print(f'\n"6, 30" pairs: {cnt_6_30}')

# Hypothesis H1: op5 = (type, value), op4 = (arg), op6 = (arg)
# Walk and see where we end
i = 28
n = len(bnr)
ops = []
bad = []
while i + 4 <= n:
    op = struct.unpack_from('<I', bnr, i)[0]
    if op == 5:
        if i + 12 > n: break
        t = struct.unpack_from('<I', bnr, i+4)[0]
        v = struct.unpack_from('<I', bnr, i+8)[0]
        ops.append((i, 5, t, v))
        i += 12
    elif op in (4, 6, 0, 1, 2):
        if i + 8 > n: break
        a = struct.unpack_from('<I', bnr, i+4)[0]
        ops.append((i, op, a))
        i += 8
    else:
        bad.append((i, op))
        i += 4
print(f'\nH1 walk: consumed {i}/{n} bytes, {len(ops)} ops, {len(bad)} unknown')
print('first unknowns:', bad[:10])
print('\nfirst 30 decoded ops:')
for o in ops[:30]:
    print(' ', o)