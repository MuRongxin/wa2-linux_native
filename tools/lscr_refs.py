import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/2003.txt', 'rb').read()
tokens = txt.split(b',')
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]
def tok(nn): return tokens[nn].decode('shift-jis','replace')[:40] if nn < len(tokens) else 'oob'

# find all references (as u32 words) to key offsets
targets = [149552, 154968, 36772, 39696, 42380, 42500, 78948, 85924, 86148]
for tgt in targets:
    refs = [p for p in range(76, n-4, 4) if w(p) == tgt]
    print(f'target {tgt}: referenced at {refs}')

# last tokens before 149552
print('\n--- op3s 147000..149700 ---')
i = 76
while i + 12 <= n:
    op = w(i)
    if op == 5: i += 12
    elif op == 6: i += 8
    elif op == 3:
        o = i; t = w(i+4); i += 8
        if 147000 <= o <= 149700:
            print(f'  {o:6d} tok#{t:4d} {tok(t)!r}')
    elif op in (0,1,2,4): i += 8
    else: i += 4

# raw dump 149400..149600
print('\n--- raw 149400..149620 ---')
for i in range(149400, 149620, 4):
    v = w(i)
    mark = '  <6>' if v == 6 else ('  <<< ' + str(v) if v in (149552, 154968) else '')
    print(f'  {i:6d}: {v:10d}{mark}')