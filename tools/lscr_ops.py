import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]

# Walk and collect op0/op1/op2 occurrences with context
i = 28
occ = {'op0': [], 'op1': [], 'op2': []}
while i + 8 <= n:
    op = w(i)
    if op == 5:
        i += 12
    elif op == 6:
        i += 8
    elif op in (0, 1, 2):
        a = w(i+4)
        occ[f'op{op}'].append((i, a))
        i += 8
    elif op in (3, 4):
        i += 8
    else:
        i += 4

for k in occ:
    print(f'{k}: {len(occ[k])} occurrences')

# op0 subtypes
from collections import Counter
print('\nop0 arg histogram (first arg):')
c = Counter(a for _, a in occ['op0'])
for a, cnt in c.most_common(30):
    print(f'  arg0={a}: {cnt}')

print('\nop1 arg histogram:')
c = Counter(a for _, a in occ['op1'])
for a, cnt in c.most_common(20):
    print(f'  arg0={a}: {cnt}')

print('\nop2 arg histogram:')
c = Counter(a for _, a in occ['op2'])
for a, cnt in c.most_common(20):
    print(f'  arg0={a}: {cnt}')

# Detailed context for each op0 subtype: show following words
print('\n--- op0 contexts (subtype -> following words) ---')
seen = {}
for off, a in occ['op0']:
    if a not in seen: seen[a] = []
    if len(seen[a]) < 4:
        ctx = [w(off + 8 + j*4) for j in range(6)]
        seen[a].append((off, ctx))
for a in sorted(seen):
    print(f'subtype {a}:')
    for off, ctx in seen[a]:
        print(f'   at {off}: next words {ctx}')