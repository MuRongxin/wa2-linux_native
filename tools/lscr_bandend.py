import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]

def dump(lo, hi, label):
    print(f'--- {label} [{lo}..{hi}] ---')
    for i in range(lo, hi+1, 4):
        v = w(i)
        mark = '  <6>' if v == 6 else ''
        print(f'  {i:6d}: {v:10d}{mark}')

# end of band1 (last tok#267 ~36696) up to band2 start 36772
dump(36676, 36788, 'band1 end -> band2 start')

# end of band2 (~39620 tok#291) to band3 start 39696
dump(39560, 39700, 'band2 end -> band3 start')

# also markers: dump around marker 580 and 1148 to see what they are
dump(560, 600, 'marker 580')
dump(1130, 1170, 'marker 1148')