import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/2003.txt', 'rb').read()
tokens = txt.split(b',')
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]
def tok(nn): return tokens[nn].decode('shift-jis','replace')[:36] if nn < len(tokens) else 'oob'

# collect op3 refs (walk)
op3s = []
i = 76
while i + 12 <= n:
    op = w(i)
    if op == 5: i += 12
    elif op == 6: i += 8
    elif op == 3:
        op3s.append((i, w(i+4))); i += 8
    elif op in (0,1,2,4):
        a = w(i+4)
        if op == 0 and a in (1,2,4):
            i += 8
        else:
            i += 8
    else: i += 4

print('--- op3 refs after choice2 branch 125576 ---')
shown = 0
for o, t in op3s:
    if o > 125576 and shown < 30:
        print(f'  {o:6d} tok#{t:4d} {tok(t)!r}')
        shown += 1

print('\n--- tokens between 149552 and 154968 ---')
for o, t in op3s:
    if 149552 <= o < 154968:
        print(f'  {o:6d} tok#{t:4d} {tok(t)!r}')

print('\n--- tokens after 154968 ---')
for o, t in op3s:
    if o >= 154968:
        print(f'  {o:6d} tok#{t:4d} {tok(t)!r}')

print('\n--- raw tail: 154900..end ---')
for i in range(154900, n, 4):
    v = w(i)
    mark = '  <6>' if v == 6 else ''
    print(f'  {i:6d}: {v:10d}{mark}')

print('\n--- raw at A=149552 ---')
for i in range(149552, 149720, 4):
    v = w(i)
    mark = '  <6>' if v == 6 else ''
    print(f'  {i:6d}: {v:10d}{mark}')

print('\n--- raw at 36772 (choice1 target A, the (0,4) record) ---')
for i in range(36772, 36920, 4):
    v = w(i)
    mark = '  <6>' if v == 6 else ''
    print(f'  {i:6d}: {v:10d}{mark}')