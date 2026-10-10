import struct

data = open('/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/WA2.exe', 'rb').read()
print('WA2.exe size', len(data))

sig = '亜唖娃阿哀愛'.encode('shift-jis')
print('JIS sig len', len(sig))
# search for the JIS-ordered kanji signature (may be u16-LE encoded)
hits = []
i = 0
while True:
    i = data.find(sig, i)
    if i < 0:
        break
    hits.append(i)
    i += 1
print('sjis byte sig at:', hits[:10])

# also try u16 pairs (lo,hi) per char in LE
sig16 = b''.join(struct.pack('<H', (b << 8) | a) for a, b in zip(sig[0::2], sig[1::2]))
hits16 = []
i = 0
while True:
    i = data.find(sig16, i)
    if i < 0:
        break
    hits16.append(i)
    i += 1
print('u16 table sig at:', hits16[:10])