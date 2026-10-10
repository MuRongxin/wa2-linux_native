import struct

bnr = open('/home/lrxin/wa2tool/extract/script/1001.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/1001.txt', 'rb').read()

print(f'bnr: {len(bnr)} bytes, txt: {len(txt)} bytes')

# Dump first 80 u32 values
print('\n--- bnr u32 stream (first 80) ---')
vals = []
for i in range(0, min(len(bnr), 320), 4):
    v = struct.unpack_from('<I', bnr, i)[0]
    vals.append(v)
    print(f'  off={i:5d}  {v:10d}  0x{v:08X}')

# Field start offsets in txt
print('\n--- txt field byte offsets (first 20) ---')
starts = [0]
for i, b in enumerate(txt):
    if b == 0x2C:  # comma
        starts.append(i+1)
print('field offsets:', starts[:25])
print('num fields:', len(starts))

# Check if bnr values match txt field offsets
print('\n--- matching bnr values against txt offsets ---')
offs = set(starts)
matches = []
for off in range(28, min(len(bnr), 2000), 4):
    v = struct.unpack_from('<I', bnr, off)[0]
    if v in offs:
        matches.append((off, v))
print(f'matches: {len(matches)}')
for off, v in matches[:30]:
    ctx = txt[v:v+20].decode('shift-jis', 'replace')
    print(f'  bnr off={off} val={v} -> txt[{v}:]={ctx!r}')