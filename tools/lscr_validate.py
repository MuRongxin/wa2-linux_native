import struct

def validate(name):
    bnr = open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()
    n = len(bnr)
    def w(off):
        return struct.unpack_from('<I', bnr, off)[0] if off+4 <= n else None
    hdr_size = w(4); count = w(8)
    labels = []
    for k in range(count):
        base = 12 + k*8
        labels.append((w(base), w(base+4)))
    code_start = labels[0][1]
    i = code_start
    ops = 0; bad = []
    while i < n:
        op = w(i)
        if op is None: break
        if op == 6: i += 8
        elif op == 5: i += 12
        elif op in (3, 4): i += 8
        elif op == 0:
            sub = w(i+4)
            if sub == 12: i += 12
            elif sub in (1, 2): i += 16
            elif sub in (13, 4): i += 8
            else:
                bad.append((i, op, sub)); i += 8
        elif op in (1, 2): i += 8
        else:
            bad.append((i, op, None)); i += 4
        ops += 1
    status = 'CLEAN' if not bad else f'{len(bad)} BAD'
    print(f'{name}: size={n} hdr={hdr_size} labels={count} code_start={code_start} consumed={i}/{n} ops={ops} -> {status}')
    for b in bad[:8]:
        print(f'    bad at {b}')

for name in ['1001','1002','1003','1004','1005','1006','1007','1008','2003','2004','2005','2006']:
    validate(name)