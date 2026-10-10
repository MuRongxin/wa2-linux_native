p = '/home/lrxin/wa2-native/src/render/mod.rs'
s = open(p, 'rb').read()
old = b"filter(|&c| c != '" + bytes([13]) + b"')"
new = b"filter(|&c| c != 0x0D)"
if old in s:
    s = s.replace(old, new)
    open(p, 'wb').write(s)
    print('patched CR literal')
else:
    print('pattern not found; searching')
    import re
    for line in s.split(b'\n'):
        if b'cleaned: String = text.chars()' in line:
            print(repr(line))