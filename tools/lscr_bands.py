import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/2003.txt', 'rb').read()
tokens = txt.split(b',')
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]
def tok(nn):
    return tokens[nn].decode('shift-jis','replace')[:46] if nn < len(tokens) else 'oob'

# Walk to collect op3 list
op3s = []
i = 76
while i + 8 <= n:
    op = w(i)
    if op == 5: i += 12
    elif op == 6: i += 8
    elif op in (0,1,2,3,4):
        if op == 3: op3s.append((i, w(i+4)))
        i += 8
    else: i += 4

def first_toks(off, k=4):
    out = []
    for o, t in op3s:
        if o >= off:
            out.append((o, t, tok(t)))
            if len(out) >= k: break
    return out

# Branch #8 at 42208 targets (42380, 42500); #10 at 85184 -> (85924, 86148)
for label, br, A, B in [('#8', 42208, 42380, 42500), ('#10', 85184, 85924, 86148)]:
    print(f'===== {label} branch at {br}, targets {A} {B} =====')
    print('  raw before branch:', [w(br-8+j*4) for j in range(8)])
    print('  target A is values-start:', w(A-8)==6 and w(A-4)==30, f'(w(A-8)={w(A-8)}, w(A-4)={w(A-4)})')
    print('  target B is values-start:', w(B-8)==6 and w(B-4)==30, f'(w(B-8)={w(B-8)}, w(B-4)={w(B-4)})')
    print(f'  first toks after branch ({br}):')
    for o, t, s in first_toks(br, 3):
        print(f'    off={o} tok#{t} {s!r}')
    print(f'  first toks after A ({A}):')
    for o, t, s in first_toks(A, 3):
        print(f'    off={o} tok#{t} {s!r}')
    print(f'  first toks after B ({B}):')
    for o, t, s in first_toks(B, 3):
        print(f'    off={o} tok#{t} {s!r}')
    # what options precede the branch?
    print('  options before branch:')
    for o, t in op3s:
        if br - 900 < o < br:
            print(f'    off={o} tok#{t} {tok(t)!r}')
    print()

# choice1: branch 34096 targets 36772/39696; options 245/246
print('===== choice1 option tokens =====')
for t in (243, 244, 245, 246, 247, 268, 292, 318):
    print(f'  tok#{t}: {tok(t)!r}')