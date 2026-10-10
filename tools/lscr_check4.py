import struct

def load(name):
    return open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()

def walk_h1(bnr):
    """op5=(type,value), op4/op6/op3/op0/op1/op2 = 1 arg"""
    i = 28
    n = len(bnr)
    ops = []
    while i + 4 <= n:
        op = struct.unpack_from('<I', bnr, i)[0]
        if op == 5:
            if i + 12 > n: break
            t = struct.unpack_from('<I', bnr, i+4)[0]
            v = struct.unpack_from('<I', bnr, i+8)[0]
            ops.append((i, 5, t, v))
            i += 12
        elif op <= 64:
            if i + 8 > n: break
            a = struct.unpack_from('<I', bnr, i+4)[0]
            ops.append((i, op, a))
            i += 8
        else:
            print(f'  unparsed at {i}: {op}')
            break
    return ops, i

bnr = load('1008')
print(f'1008.bnr size {len(bnr)}')
ops, end = walk_h1(bnr)
print(f'walk ended at {end}/{len(bnr)}, ops={len(ops)}')

# Search for values in 190..220 in op args
print('\n--- op args in [190, 220] ---')
found = []
for entry in ops:
    if len(entry) == 4:  # op5 (type value)
        off, op, t, v = entry
        if 190 <= v <= 220:
            found.append(entry)
    else:
        off, op, a = entry
        if 190 <= a <= 220:
            found.append(entry)
print(f'found {len(found)}:')
for f in found[:40]:
    print(' ', f)

# Also dump ops near where these cluster
if found:
    lo = found[0][0]
    print(f'\n--- ops around offset {lo} ---')
    idx = [i for i, e in enumerate(ops) if e[0] == lo][0]
    for e in ops[max(0, idx-6):idx+10]:
        print(' ', e)