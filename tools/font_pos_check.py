import struct

# atlas
d = open('/home/lrxin/wa2tool/fonts/jp_body80.tga', 'rb').read()
w = struct.unpack_from('<H', d, 12)[0]
h = struct.unpack_from('<H', d, 14)[0]
idlen = d[0]; cmap_len = struct.unpack_from('<H', d, 5)[0]; cb = (d[7]//8) or 1
pos = 18 + idlen + cmap_len*cb
cell = 40; cols = w // cell; flip = (d[17] & 0x20) == 0

def get_cell(idx):
    row = idx // cols; col = idx % cols
    px = []
    for y in range(cell):
        sy = (h-1-(row*cell+y)) if flip else (row*cell+y)
        for x in range(cell):
            px.append(d[pos + sy*w + col*cell + x])
    return px

def art(idx, label):
    c = get_cell(idx)
    nz = sum(1 for p in c if p > 0)
    print(f'--- {label}: cell {idx}, nonzero={nz} ---')
    for y in range(0, cell, 4):
        print('  ' + ''.join('#' if c[y*cell+x] > 0 else '.' for x in range(0, cell, 2)))

cs = open('/tmp/wa2_sjis_charset.txt', encoding='utf-8')
charset = cs.read()
print('charset len:', len(charset))
for ch in [' ', '!', 'A', '亜']:
    i = charset.find(ch)
    print(f'char {ch!r} at index {i}')

art(0, "start_pos=0 for ' ' (expect empty)")
art(1, "start_pos=0 for '!' (expect thin bar)")
art(748, "start_pos=748 for ' ' (?)")
art(749, "start_pos=748 for '!' (?)")
iA = charset.find('A')
art(iA, f"start_pos=0 for 'A'")
art(748 + iA, f"start_pos=748 for 'A'")