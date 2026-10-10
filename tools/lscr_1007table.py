import struct
bnr = open('/home/lrxin/wa2tool/extract/script/1007.bnr', 'rb').read()
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]
# find op0 subtype 5 (aligned)
for p in range(28, n-8, 4):
    if w(p) == 0 and w(p+4) == 5:
        print(f'(0,5) at {p}: following words:')
        for q in range(p, p+48, 4):
            v = w(q)
            note = ' <offset>' if 28 < v < n else ''
            print(f'   {q}: {v}{note}')
        print()
# also op0 subtype 14
for p in range(28, n-8, 4):
    if w(p) == 0 and w(p+4) == 14:
        print(f'(0,14) at {p}: following:')
        for q in range(p, p+32, 4):
            v = w(q)
            note = ' <offset>' if 28 < v < n else ''
            print(f'   {q}: {v}{note}')
        print()