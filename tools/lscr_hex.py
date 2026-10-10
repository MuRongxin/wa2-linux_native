bnr = open('/home/lrxin/wa2tool/extract/script/1002.bnr', 'rb').read()
print('--- raw hex 103984..104240 ---')
for i in range(103984, 104240, 16):
    chunk = bnr[i:i+16]
    hexs = ' '.join(f'{b:02x}' for b in chunk)
    print(f'{i:6d}: {hexs}')

import struct
print('\n--- u32 stream from 103984 (4-aligned) ---')
for i in range(103984, 104100, 4):
    print(f'  {i}: {struct.unpack_from("<I", bnr, i)[0]}')