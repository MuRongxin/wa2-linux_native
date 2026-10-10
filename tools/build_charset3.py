s = open('/tmp/wa2_font_charset.txt', encoding='utf-8').read()
junk = s.find('1516171819')
core = s[:junk] if junk > 0 else s
print('table entries:', len(core))

NCELL = 7680
slots = ['\uFFFD'] * NCELL          # placeholder for unknown cells
# ASCII block at 0..94
for k, c in enumerate(range(0x20, 0x7F)):
    slots[k] = chr(c)
# table entries at cell = 224 + idx
for idx, ch in enumerate(core):
    cell = 224 + idx
    if cell < NCELL:
        slots[cell] = ch
charset = ''.join(slots)
open('/tmp/wa2_font_charset3.txt', 'w', encoding='utf-8').write(charset)
print('written charset3, len', len(charset))
for ch in [' ', '!', 'A', 'と', '。', 'あ', '亜', '春', '小', '雪', '菜', '千', '晶', '和', '泉', '希']:
    i = charset.find(ch)
    print(f'  {ch!r} -> cell {i}')