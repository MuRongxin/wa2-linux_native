import struct

jp = open('/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/WA2.exe', 'rb').read()

# find start: pair (0x8140, 0x81AC)
pair = struct.pack('<HH', 0x8140, 0x81AC)
hits = []
i = 0
while True:
    i = jp.find(pair, i)
    if i < 0:
        break
    hits.append(i)
    i += 1
print('JP exe start-pair hits:', hits)

# candidate bases: for each hit, dump the following u16s
for h in hits[:6]:
    print(f'--- at {h} ---')
    vals = [struct.unpack_from('<H', jp, h + k*2)[0] for k in range(20)]
    print(' ', [hex(v) for v in vals])


def build(base):
    def sjis_char(hi, lo):
        try:
            return bytes([hi, lo]).decode('shift-jis')
        except Exception:
            return None
    chars = []
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
    return ''.join(chars)


for h in hits[:3]:
    s = build(h)
    print(f'base {h}: {len(s)} chars; 亜@{s.find("亜")} あ@{s.find("あ")} 春@{s.find("春")} 晶@{s.find("晶")} 和@{s.find("和")}')
    open(f'/tmp/wa2_jp_table_{h}.txt', 'w', encoding='utf-8').write(s)