import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/2003.txt', 'rb').read()
tokens = txt.split(b',')
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]
def tok(nn): return tokens[nn].decode('shift-jis','replace')[:34] if nn < len(tokens) else 'oob'

# raw scan: find all (4,208) and (2,0) and (0,2,..) - dump with record context
print('--- all (4,208) option actions ---')
for p in range(76, n-16, 4):
    if w(p) == 4 and w(p+4) == 208:
        # find the token in the same record (search backwards for 3,N)
        t = None
        for q in range(p-1, max(76, p-40), -1):
            if w(q-1) == 3 if q >= 4 else False:
                pass
        # simpler: look back up to 40 bytes for pattern 3, N
        for q in range(p-40, p, 4):
            if q >= 4 and w(q) == 3 and 0 < w(q+4) < len(tokens) and w(q-4) == 6 or (q >= 8 and w(q) == 3 and w(q-8) == 30):
                pass
        print(f'  (4,208) at {p}')

print('\n--- all (2,0) ---')
for p in range(76, n-8, 4):
    if w(p) == 2 and w(p+4) == 0:
        print(f'  (2,0) at {p}')

# For each branch, find nearest preceding (4,208) group
print('\n--- branches and nearest preceding (4,208) ---')
branches = [34096, 42208, 60332, 85184, 125576]
opt208 = [p for p in range(76, n-8, 4) if w(p) == 4 and w(p+4) == 208]
for b in branches:
    prev = [p for p in opt208 if p < b]
    nearest = prev[-3:] if prev else []
    print(f'  branch@{b}: nearest (4,208) at {nearest}')
    # find tokens in those records
    for p in nearest:
        # search back for 3,N within 60 bytes
        found = None
        for q in range(p, max(76, p-60), -4):
            if w(q) == 3 and w(q+4) < len(tokens):
                found = w(q+4); break
        print(f'      (4,208)@{p} token={found} {tok(found)!r}' if found else f'      (4,208)@{p} token=?')