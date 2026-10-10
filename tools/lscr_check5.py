import struct, glob, os

def load_bnr(name):
    return open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()

def load_txt(name):
    return open(f'/home/lrxin/wa2tool/extract/script/{name}.txt', 'rb').read()

def walk(bnr):
    i = 28; n = len(bnr); ops = []
    while i + 4 <= n:
        op = struct.unpack_from('<I', bnr, i)[0]
        if op == 5:
            if i + 12 > n: break
            t = struct.unpack_from('<I', bnr, i+4)[0]
            v = struct.unpack_from('<I', bnr, i+8)[0]
            ops.append((i, 5, t, v)); i += 12
        elif op <= 64:
            if i + 8 > n: break
            a = struct.unpack_from('<I', bnr, i+4)[0]
            ops.append((i, op, a)); i += 8
        else:
            ops.append((i, op, None)); i += 4  # raw unknown
    return ops, i

for name in ['1001', '1002', '1003', '1008', '2003']:
    bnr = load_bnr(name)
    txt = load_txt(name)
    tokens = txt.split(b',')
    ntok = len(tokens)
    ops, end = walk(bnr)
    # collect args
    a3 = [e[2] for e in ops if e[1] == 3 and e[2] is not None]
    a4 = [e[2] for e in ops if e[1] == 4 and e[2] is not None]
    a6 = [e[2] for e in ops if e[1] == 6 and e[2] is not None]
    a5i = [e[3] for e in ops if e[1] == 5 and e[2] == 3]
    a5f = [e[3] for e in ops if e[1] == 5 and e[2] == 4]
    print(f'=== {name} === bnr={len(bnr)} tokens={ntok} ops={len(ops)} walkend={end}')
    for lbl, arr in [('3,x', a3), ('4,x', a4), ('6,x', a6), ('5,3,x', a5i), ('5,4,x', a5f)]:
        if arr:
            print(f'  {lbl}: n={len(arr)} min={min(arr)} max={max(arr)}  dist_sample={sorted(set(arr))[:12]}')
        else:
            print(f'  {lbl}: none')
    print()