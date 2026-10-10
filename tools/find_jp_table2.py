import struct

cn = open('/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/WA2_chs.exe', 'rb').read()
jp = open('/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/WA2.exe', 'rb').read()

base = 656576
print('=== CN table first 24 u16 at 656576 ===')
vals = [struct.unpack_from('<H', cn, base + i*2)[0] for i in range(24)]
print([hex(v) for v in vals])

# locate 亜 (0x889F) in the CN table
target = struct.pack('<H', 0x889F)
idx = cn.find(target, base, base + 8000)
print('0x889F at', idx, '(offset from base:', (idx - base) if idx > 0 else None, ')')

# search JP exe for 0x889F followed by 0x88FC (first kanji range)
pair = struct.pack('<HH', 0x889F, 0x88FC)
hits = []
i = 0
while True:
    i = jp.find(pair, i)
    if i < 0:
        break
    hits.append(i)
    i += 1
print('JP exe hits for [0x889F,0x88FC]:', hits[:10])

# also in CN for sanity
hits = []
i = 0
while True:
    i = cn.find(pair, i)
    if i < 0:
        break
    hits.append(i)
    i += 1
print('CN exe hits:', hits[:10])