import struct
from PIL import Image

d = open('/home/lrxin/wa2tool/fonts/jp_body80.tga', 'rb').read()
w = struct.unpack_from('<H', d, 12)[0]
h = struct.unpack_from('<H', d, 14)[0]
idlen = d[0]; cmap_len = struct.unpack_from('<H', d, 5)[0]; cb = (d[7]//8) or 1
pos = 18 + idlen + cmap_len*cb
cell = 40; cols = w // cell; flip = (d[17] & 0x20) == 0

def sheet(first, count, ncols, out, scale=2):
    rows = (count + ncols - 1) // ncols
    img = Image.new('L', (ncols*cell, rows*cell), 0)
    px = img.load()
    for k in range(count):
        idx = first + k
        row = idx // cols; col = idx % cols
        r = k // ncols; c = k % ncols
        for y in range(cell):
            sy = (h-1-(row*cell+y)) if flip else (row*cell+y)
            for x in range(cell):
                v = d[pos + sy*w + col*cell + x]
                px[c*cell+x, r*cell+y] = v
    img = img.resize((img.width*scale, img.height*scale), Image.NEAREST)
    img.save(out)
    print(f'{out}: cells {first}..{first+count-1} ({ncols} cols), glyph idx: row-major from {first}')

sheet(0, 480, 20, '/tmp/sheet_a.png', 2)
sheet(480, 480, 20, '/tmp/sheet_b.png', 2)