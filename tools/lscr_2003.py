import struct

def load(name):
    bnr = open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()
    txt = open(f'/home/lrxin/wa2tool/extract/script/{name}.txt', 'rb').read()
    return bnr, txt.split(b',')

bnr, tokens = load('2003')
print(f'2003: bnr={len(bnr)} tokens={len(tokens)}')

# find choice tokens
for j, t in enumerate(tokens):
    s = t.decode('shift-jis', 'replace')
    if s.strip().startswith(('１．', '２．', '３．')) or '苦手だ' in s or '心地良い' in s or '電話をかける' in s or 'メールを送る' in s:
        print(f'tok#{j}: {s[:40]!r}')

# find CATCH tokens
print('--- CATCH tokens ---')
for j, t in enumerate(tokens):
    s = t.decode('shift-jis', 'replace').strip()
    if s in ('CATCH', 'CATCH2', 'CATCH3'):
        nxt = tokens[j+1].decode('shift-jis', 'replace').strip() if j+1 < len(tokens) else ''
        print(f'tok#{j}: {s} -> {nxt!r}')
        if j > 700:
            break