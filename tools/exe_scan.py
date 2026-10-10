import re

data = open('/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/WA2.exe', 'rb').read()
print(f'WA2.exe size: {len(data)}')

# search for ASCII/Shift-JIS strings containing sprite-like names
def find_all(pat, limit=12):
    out = []
    for m in re.finditer(pat, data):
        out.append(m.start())
        if len(out) >= limit:
            break
    return out

for pat in [rb'har002101', rb'kaz001', rb'ioo013', rb'set001', rb'izu001', rb'\.tga', rb'char\.pak', rb'CHR', rb'chr']:
    hits = find_all(pat)
    print(f'{pat}: {len(hits)} hits at {hits[:8]}')
    if hits and pat in (rb'\.tga',):
        # print context of first hits
        for h in hits[:5]:
            lo = max(0, h - 40)
            ctx = data[lo:h + 20]
            try:
                print('   ctx:', ctx.decode('shift-jis', 'replace').replace('\x00', '|'))
            except Exception:
                print('   ctx:', ctx)