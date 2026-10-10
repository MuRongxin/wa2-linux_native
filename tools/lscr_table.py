import struct

def dump_region(name, lo, hi):
    bnr = open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()
    n = len(bnr)
    def w(off): return struct.unpack_from('<I', bnr, off)[0]
    print(f'--- {name} raw [{lo}..{hi}] (file n={n}) ---')
    for i in range(lo, hi, 4):
        v = w(i)
        note = ''
        if v == 6: note = '  <6>'
        elif 76 < v < n: note = '  <-- offset!'
        print(f'  {i:6d}: {v:12d}{note}')

dump_region('1002', 103990, 104280)
print()
dump_region('1007', 223610, 223900)