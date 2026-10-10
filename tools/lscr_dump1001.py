import struct

bnr = open('/home/lrxin/wa2tool/extract/script/1001.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/1001.txt', 'rb').read()
tokens = txt.split(b',')

def tok_str(n):
    if 0 <= n < len(tokens):
        s = tokens[n].decode('shift-jis', 'replace').replace('\n', '\\n')
        return s[:48]
    return '<oob>'

i = 28; n = len(bnr); step = 0
while i + 4 <= n and step < 400:
    op = struct.unpack_from('<I', bnr, i)[0]
    if op == 5:
        t = struct.unpack_from('<I', bnr, i+4)[0]
        v = struct.unpack_from('<I', bnr, i+8)[0]
        fv = struct.unpack('<f', struct.pack('<I', v))[0]
        extra = f'   float={fv}' if t == 4 else ''
        print(f'{i:5d}: 5, {t}, {v}{extra}')
        i += 12
    elif op <= 64:
        a = struct.unpack_from('<I', bnr, i+4)[0]
        if op == 3:
            print(f'{i:5d}: 3, {a:4d}   TEXT[{a}] = {tok_str(a)!r}')
        else:
            print(f'{i:5d}: {op}, {a}')
        i += 8
    else:
        print(f'{i:5d}: RAW? {op}')
        i += 4
    step += 1