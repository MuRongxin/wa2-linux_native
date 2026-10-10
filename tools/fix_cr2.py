p = '/home/lrxin/wa2-native/src/render/mod.rs'
s = open(p, 'rb').read()
old = b"filter(|&c| c != 0x0D)"
new = b"filter(|&c| c != '\\r')"
s = s.replace(old, new)
open(p, 'wb').write(s)
print('done')