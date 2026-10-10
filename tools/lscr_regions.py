import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]

def dump(lo, hi, label):
    print(f'--- {label}: words [{lo}..{hi}] ---')
    for i in range(lo, hi+1, 4):
        v = w(i)
        mark = '  <- 6' if v == 6 else ''
        print(f'  {i:6d}: {v:10d} 0x{v:08X}{mark}')

dump(0, 24, 'file header')
dump(284, 312, 'around target 300')
dump(36740, 36784, 'around target A 36772')
dump(39660, 39704, 'around target B 39696')