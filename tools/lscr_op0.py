import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]

# proper walk from code start 76
i = 76
real = []
while i + 8 <= n:
    op = w(i)
    if op == 5:
        i += 12
    elif op == 6:
        i += 8
    elif op in (0, 1, 2):
        real.append((i, op, w(i+4)))
        i += 8
    elif op in (3, 4):
        i += 8
    else:
        # could be a raw value in a context we don't understand; skip
        real.append((i, op, None))
        i += 4

print('=== ALL op0 subtype 2 occurrences (full context) ===')
cnt = 0
for off, op, a in real:
    if op == 0 and a == 2:
        cnt += 1
        lo = off - 12; hi = off + 40
        ws = [w(j) for j in range(lo, hi, 4)]
        print(f'#{cnt} at {off}: ... ' + ' '.join(str(x) for x in ws))
print(f'total: {cnt}')

print('\n=== ALL op0 subtype 1 occurrences ===')
for off, op, a in real:
    if op == 0 and a == 1:
        ws = [w(j) for j in range(off-12, off+24, 4)]
        print(f'at {off}: ... ' + ' '.join(str(x) for x in ws))

print('\n=== ALL op2 occurrences ===')
for off, op, a in real:
    if op == 2:
        ws = [w(j) for j in range(off-12, off+24, 4)]
        print(f'at {off}: arg={a} ... ' + ' '.join(str(x) for x in ws))