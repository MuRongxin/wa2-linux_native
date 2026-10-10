import struct
from PIL import Image

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

def sheet(first, count, ncols, out, scale=3):
    rows = (count + ncols - 1) // ncols
    img = Image.new('L', (ncols*cell, rows*cell), 0)
    px = img.load()
    for k in range(count):
        idx = first + k
        c = get_cell(idx)
        r = k // ncols; cc = k % ncols
        for y in range(cell):
            for x in range(cell):
                px[cc*cell+x, r*cell+y] = c[y*cell+x]
    img = img.resize((img.width*scale, img.height*scale), Image.NEAREST)
    img.save(out)
    print(f'{out}: cells {first}..{first+count-1}, {ncols} cols; row-major; idx = {first} + row*{ncols} + col')

# kana area probe: 380..459 (8 cols)
sheet(380, 80, 8, '/tmp/kana_380.png', 3)
# symbols area: 224..303 (8 cols)
sheet(224, 80, 8, '/tmp/sym_224.png', 3)