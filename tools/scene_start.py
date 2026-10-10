import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/2003.txt', 'rb').read()
tokens = txt.split(b',')
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]
def tok(nn): return tokens[nn].decode('shift-jis','replace')[:26] if nn < len(tokens) else 'oob'

# linear dump from entry 1792 for 2600 bytes
i = 1792
end = 1792 + 3200
rec_ints = []
print(f'dump 2003 from {i} to {end}')
while i + 12 <= n and i < end:
    op = w(i)
    if op == 6:
        print(f'\nK{i}: kind={w(i+4)}')
        rec_ints = []
        i += 8
    elif op == 5:
        t = w(i+4); v = w(i+8)
        if t == 4:
            vv = struct.unpack('<f', struct.pack('<I', v))[0]
            print(f'  f={vv:.1f}', end='')
        else:
            print(f'  i={v}', end='')
        rec_ints.append(v)
        i += 12
    elif op == 3:
        print(f"  tok#{w(i+4)}({tok(w(i+4))!r})", end='')
        i += 8
    elif op == 4:
        act = w(i+4)
        print(f'  ACT{act}', end='')
        if act == 138 and len(rec_ints) >= 5:
            print(f'    <== SPRITE char={rec_ints[-5]} p4={rec_ints[-1]}', end='')
        i += 8
    elif op == 0:
        sub = w(i+4)
        if sub == 12:
            print(f'  GOTO{w(i+8)}', end=''); i += 12
        elif sub in (1, 2):
            print(f'  (0,{sub},{w(i+8)},{w(i+12)})', end=''); i += 16
        else:
            print(f'  (0,{sub})', end=''); i += 8
    elif op in (1, 2):
        print(f'  ({op},{w(i+4)})', end=''); i += 8
    else:
        print(f'  RAW{w(i)}', end=''); i += 4