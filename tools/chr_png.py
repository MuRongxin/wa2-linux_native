import struct, os
from PIL import Image

path = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak'
f = open(path, 'rb')
hdr = f.read(16)
n = struct.unpack_from('<I', hdr, 12)[0]
entries = {}
order = []
for i in range(n):
    f.seek(16 + i*44)
    e = f.read(44)
    comp = struct.unpack_from('<I', e, 0)[0]
    name = e[4:28].split(b'\x00')[0].decode('shift-jis', 'replace')
    off = struct.unpack_from('<I', e, 36)[0]
    size = struct.unpack_from('<I', e, 40)[0]
    entries[name] = (comp, off, size)
    order.append(name)

N, F, TH = 4096, 18, 2
def lzss(data, out_len):
    text = bytearray(N + F - 1)
    r = N - F; flags = 0; idx = 0; out = bytearray()
    while True:
        flags >>= 1
        if not (flags & 256):
            if idx >= len(data): break
            flags = data[idx] | 0xFF00; idx += 1
        if flags & 1:
            if idx >= len(data): break
            c = data[idx]; idx += 1
            text[r] = c; r = (r+1) & (N-1); out.append(c)
            if len(out) >= out_len: break
        else:
            if idx+1 >= len(data): break
            i0 = data[idx]; j0 = data[idx+1]; idx += 2
            i = i0 | ((j0 & 0xF0) << 4)
            j = (j0 & 0x0F) + TH
            for k in range(j+1):
                c = text[(i+k) & (N-1)]
                text[r] = c; r = (r+1) & (N-1); out.append(c)
                if len(out) >= out_len: return bytes(out)
    return bytes(out)

def get_raw(name):
    comp, off, size = entries[name]
    f.seek(off)
    raw = f.read(size)
    if comp:
        hlen, olen = struct.unpack_from('<II', raw, 0)
        return lzss(raw[8:], olen)
    return raw

def tga_to_png(name, out_png, scale=None, bg=(30, 30, 40)):
    d = get_raw(name)
    w = struct.unpack_from('<H', d, 12)[0]
    h = struct.unpack_from('<H', d, 14)[0]
    depth = d[16]; desc = d[17]
    idlen = d[0]
    cmap_type = d[1]; img_type = d[2]
    cmap_len = struct.unpack_from('<H', d, 5)[0]
    cmap_depth = d[7]
    pos = 18 + idlen
    # colormap
    cmap = None
    if cmap_type:
        cb = (cmap_depth // 8) or 1
        cmap = d[pos:pos + cmap_len * cb]
        pos += cmap_len * cb
    pixel_bytes = (depth // 8) or 1
    img = Image.new('RGBA', (w, h), bg + (255,))
    px = img.load()
    flip = (desc & 0x20) == 0
    data = d[pos:pos + w*h*pixel_bytes]
    for y in range(h):
        sy = (h-1-y) if flip else y
        row = sy * w * pixel_bytes
        for x in range(w):
            o = row + x*pixel_bytes
            if img_type in (1, 9):
                ci = data[o]
                if cmap is not None and ci * 4 + 3 < len(cmap):
                    b, g, r, a = cmap[ci*4:ci*4+4]
                    px[x, y] = (r, g, b, a)
                elif cmap is not None and ci * 3 + 2 < len(cmap):
                    b, g, r = cmap[ci*3:ci*3+3]
                    px[x, y] = (r, g, b, 255)
            elif img_type in (2, 10):
                if pixel_bytes == 4:
                    b, g, r, a = data[o], data[o+1], data[o+2], data[o+3]
                    px[x, y] = (r, g, b, a)
                elif pixel_bytes == 3:
                    b, g, r = data[o], data[o+1], data[o+2]
                    px[x, y] = (r, g, b, 255)
    if scale:
        img = img.resize((w*scale, h*scale), Image.NEAREST)
    img.save(out_png)
    return w, h, depth, desc

os.makedirs('/tmp/wa2_chr_png', exist_ok=True)
# extract a few candidates for visual ID
tests = [
    'ioo013101.tga', 'ioo013102.tga', 'ioo013103.tga',
    'kaz001101.tga', 'kaz001104.tga', 'kaz001109.tga',
    'set001102.tga', 'set001106.tga', 'set001321.tga',
    'izu001000.tga', 'izu001001.tga', 'izu001002.tga',
    'har002101.tga',
]
for t in tests:
    if t in entries:
        try:
            w, h, dep, desc = tga_to_png(t, f'/tmp/wa2_chr_png/{t[:-4]}.png')
            print(f'{t}: {w}x{h} depth={dep} desc=0x{desc:02x}')
        except Exception as ex:
            print(f'{t}: FAIL {ex}')
    else:
        print(f'{t}: not found')