import struct

data = open('/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/WA2_chs.exe', 'rb').read()
off = 656576

def sjis_char(hi, lo):
    try:
        return bytes([hi, lo]).decode('shift-jis')
    except Exception:
        return None

chars = []
j = 0
while j < 4000:
    p = off + j*2
    if p + 3 >= len(data):
        break
    lo1 = data[p]; hi1 = data[p+1]
    lo2 = data[p+2]; hi2 = data[p+3]
    code1 = lo1 | (hi1 << 8)
    code2 = lo2 | (hi2 << 8)
    if code1 == 0 and code2 == 0:
        break
    if code2 > code1:
        # range pair: expand all Shift-JIS codes from (hi1,lo1) to (hi2,lo2)
        for hi in range(hi1, hi2+1):
            lo_s = lo1 if hi == hi1 else 0x40
            lo_e = lo2 if hi == hi2 else 0xFC
            for lo in range(lo_s, lo_e+1):
                if lo == 0x7F:
                    continue
                c = sjis_char(hi, lo)
                if c is not None:
                    chars.append(c)
        j += 2
    else:
        c = sjis_char(hi1, lo1)
        if c is not None:
            chars.append(c)
        j += 1

s = ''.join(chars)
print('chars:', len(s))
for ch in [' ', '!', 'A', 'あ', '亜', '春', '雪', '菜', '和', '泉', '千', '晶']:
    print(f'  {ch!r} at {s.find(ch)}')
open('/tmp/wa2_font_charset.txt', 'w', encoding='utf-8').write(s)
print('written /tmp/wa2_font_charset.txt')