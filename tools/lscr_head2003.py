import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
n = len(bnr)
print(f'2003.bnr size {n}')

def w(off):
    return struct.unpack_from('<I', bnr, off)[0]

print('--- first 64 words ---')
for i in range(28, 28+64*4, 4):
    print(f'  {i:5d}: {w(i):12d} 0x{w(i):08X}')

print('\n--- what is at early targets? ---')
for tgt in [76, 300, 504, 556, 1124, 1712, 1792, 1796]:
    if tgt + 32 <= n:
        ctx = [w(tgt + j*4) for j in range(8)]
        # is a record start nearby?
        print(f'  {tgt:5d}: {ctx}')

print('\n--- check 1792..1900 words ---')
for i in range(1788, 1900, 4):
    print(f'  {i:5d}: {w(i):12d}')