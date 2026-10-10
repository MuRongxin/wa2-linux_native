import struct

jp = open('/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/WA2.exe', 'rb').read()
base = 656064

def sjis_char(hi, lo):
    try:
        return bytes([hi, lo]).decode('shift-jis')
    except Exception:
        return None

cells = []
j = 0
while j < 6000:
    p = base + j*2
    if p + 3 >= len(jp):
        break
    lo1 = jp[p]; hi1 = jp[p+1]
    lo2 = jp[p+2]; hi2 = jp[p+3]
    code1 = lo1 | (hi1 << 8)
    code2 = lo2 | (hi2 << 8)
    if code1 == 0 and code2 == 0:
        break
    if code2 > code1 and hi2 >= hi1:
        for hi in range(hi1, hi2+1):
            lo_s = lo1 if hi == hi1 else 0x40
            lo_e = lo2 if hi == hi2 else 0xFC
            for lo in range(lo_s, lo_e+1):
                c = sjis_char(hi, lo)
                cells.append(c if c is not None else '\uFFFD')
        j += 2
    else:
        c = sjis_char(hi1, lo1)
        cells.append(c if c is not None else '\uFFFD')
        j += 1

core = ''.join(cells)
print('core cells:', len(core))
for ch in ['あ', '亜', '春', '小', '晶', '和', '雪']:
    print(f'  {ch} at {core.find(ch)}')

NCELL = 7680
slots = ['\uFFFD'] * NCELL
for k, c in enumerate(range(0x20, 0x7F)):
    slots[k] = chr(c)
for idx, ch in enumerate(core):
    cell = 224 + idx
    if cell < NCELL:
        slots[cell] = ch
charset = ''.join(slots)
open('/tmp/wa2_font_charset4.txt', 'w', encoding='utf-8').write(charset)
print('written charset4')
for ch in [' ', 'A', 'あ', 'と', '亜', '春', '小', '雪', '菜', '千', '晶', '和', '泉', '希']:
    print(f'  {ch!r} -> cell {charset.find(ch)}')