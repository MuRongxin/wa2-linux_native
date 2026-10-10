import struct
from collections import Counter

path = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak'
f = open(path, 'rb')
hdr = f.read(16)
n = struct.unpack_from('<I', hdr, 12)[0]
entries = []
for i in range(n):
    f.seek(16 + i*44)
    e = f.read(44)
    comp = struct.unpack_from('<I', e, 0)[0]
    name = e[4:28].split(b'\x00')[0].decode('shift-jis', 'replace')
    off = struct.unpack_from('<I', e, 36)[0]
    size = struct.unpack_from('<I', e, 40)[0]
    entries.append((name, comp, off, size))

prefixes = Counter(nm[:3] for nm, _, _, _ in entries)
print('char prefixes:', dict(prefixes))
for p in prefixes:
    samples = [nm for nm, _, _, _ in entries if nm.startswith(p)][:6]
    print(f'  {p}: {samples}')

# extract one sprite per prefix, decompress, save raw TGA, convert to PNG
import io, zlib

def decompress(raw, orig):
    return None  # placeholder

# use the project's own LZSS? implement quickly (same algorithm)
N, F, TH = 4096, 18, 2
def lzss(data, out_len):
    text = bytearray(N + F - 1)
    r = N - F
    flags = 0
    idx = 0
    out = bytearray()
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

def extract(name):
    for nm, comp, off, size in entries:
        if nm == name:
            f.seek(off)
            raw = f.read(size)
            if comp:
                hlen, olen = struct.unpack_from('<II', raw, 0)
                return lzss(raw[8:], olen)
            return raw
    return None

import os
os.makedirs('/tmp/wa2_chr', exist_ok=True)
for pick in ['ioo013101.tga', 'har002101.tga', 'ham002001.tga', 'aco001001.tga', 'hon018001.tga']:
    d = extract(pick)
    if d:
        open(f'/tmp/wa2_chr/{pick}', 'wb').write(d)
        w = struct.unpack_from('<H', d, 12)[0]
        h = struct.unpack_from('<H', d, 14)[0]
        dep = d[16]; desc = d[17]
        print(f'{pick}: {w}x{h} depth={dep} desc=0x{desc:02x} len={len(d)}')