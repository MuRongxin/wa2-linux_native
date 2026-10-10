import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/2003.txt', 'rb').read()
tokens = txt.split(b',')
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]
def tok(nn): return tokens[nn].decode('shift-jis','replace')[:40] if nn < len(tokens) else 'oob'

# Walk from 76, collect ALL control ops with offsets
i = 76
op3s = []; jumps12 = []; ret13 = []; sub1 = []; sub4 = []; br2 = []
while i + 12 <= n:
    op = w(i)
    if op == 5: i += 12
    elif op == 6: i += 8
    elif op == 3:
        op3s.append((i, w(i+4))); i += 8
    elif op in (0,1,2,4):
        a = w(i+4)
        if op == 0:
            if a == 12: jumps12.append((i, w(i+8)))
            elif a == 13: ret13.append(i)
            elif a == 1: sub1.append((i, w(i+8), w(i+12)))
            elif a == 4: sub4.append((i, w(i+8), w(i+12)))
            elif a == 2: br2.append((i, w(i+8), w(i+12)))
        i += 8
    else:
        i += 4

print(f'jumps12: {len(jumps12)}')
for off, tgt in jumps12:
    # token context at jump and target
    print(f'  jump at {off} -> {tgt}')

print(f'\nreturns (0,13): {len(ret13)} at {ret13}')

print(f'\n(0,1,X,Y) markers: {sub1}')
print(f'\n(0,4,X,Y): {sub4}')

# op3 offsets vs token ids: list around choice1 bands
print('\n--- op3 token sequence around choice1 (offset 33900..44000) ---')
seq = [(o, t) for o, t in op3s if 33000 <= o <= 44000]
prev = None
for o, t in seq:
    gap = '' if prev is None or t == prev + 1 else '  <<< GAP'
    print(f'  {o:6d} tok#{t:4d} {tok(t)!r}{gap}')
    prev = t