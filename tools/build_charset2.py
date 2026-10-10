s = open('/tmp/wa2_font_charset.txt', encoding='utf-8').read()
# locate junk start: the "151617" artifact
j = s.find('1516171819')
print('junk starts at', j)
print('before junk:', repr(s[j-30:j]))
if j > 0:
    core = s[:j]
else:
    core = s
print('core len:', len(core))
ascii_block = ''.join(chr(c) for c in range(0x20, 0x7F))
charset = ascii_block + core
print('combined len:', len(charset), '(atlas has 7680 cells)')
for ch in [' ', '!', 'A', 'あ', '亜', '春', '小', '雪', '菜', '通']:
    print(f'  {ch!r} at {charset.find(ch)}')
open('/tmp/wa2_font_charset2.txt', 'w', encoding='utf-8').write(charset)
print('written /tmp/wa2_font_charset2.txt')