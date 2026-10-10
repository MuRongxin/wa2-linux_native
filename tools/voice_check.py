import struct

bnr = open('/home/lrxin/wa2tool/extract/script/1008.bnr', 'rb').read()
txt = open('/home/lrxin/wa2tool/extract/script/1008.txt', 'rb').read()
tokens = txt.split(b',')
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0]
def tok(nn): return tokens[nn].decode('shift-jis','replace')[:30] if nn < len(tokens) else 'oob'

# walk with records; print records between first op3 of tok#195 and tok#214
i = 28
rec = None        # (off, kind, vals)
recs = []
while i + 12 <= n:
    op = w(i)
    if op == 6:
        rec = (i, w(i+4), []); recs.append(rec); i += 8
    elif op == 5:
        t = w(i+4); v = w(i+8)
        if rec: rec[2].append(('f' if t == 4 else 'i', v))
        i += 12
    elif op == 3:
        if rec: rec[2].append(('tok', w(i+4)))
        i += 8
    elif op == 4:
        if rec: rec[2].append(('act', w(i+4)))
        i += 8
    elif op == 0:
        sub = w(i+4)
        i += 12 if sub == 12 else (16 if sub in (1,2) else 8)
    elif op in (1, 2):
        i += 8
    else:
        i += 4

# find records whose tok in 195..216
print('--- records touching tok#195..216 ---')
for off, kind, vals in recs:
    toks = [v for k, v in vals if k == 'tok']
    if any(195 <= t <= 216 for t in toks):
        desc = []
        for k, v in vals:
            if k == 'tok':
                desc.append(f"tok#{v}({tok(v)!r})")
            elif k == 'act':
                desc.append(f"ACT{v}")
            else:
                desc.append(f"{k}={v}")
        print(f'  {off:6d} K{kind}: ' + ', '.join(desc))