import struct

bnr = open('/home/lrxin/wa2tool/extract/script/2003.bnr', 'rb').read()
n = len(bnr)
def w(off): return struct.unpack_from('<I', bnr, off)[0] if off + 4 <= n else -1

# raw scan for control patterns at any 4-aligned position
print('--- raw scan: (0,12,target) jumps ---')
for p in range(76, n-12, 4):
    if w(p) == 0 and w(p+4) == 12:
        t = w(p+8)
        ok = t < n
        vs = ok and w(t-8) == 6 and w(t-4) == 30
        print(f'  at {p}: jump -> {t}  valid={ok} target_is_values_start={vs}')

print('\n--- raw scan: (0,2,A,B) branches ---')
for p in range(76, n-16, 4):
    if w(p) == 0 and w(p+4) == 2:
        A, B = w(p+8), w(p+12)
        va = A < n and w(A-8) == 6 and w(A-4) == 30
        vb = B < n and w(B-8) == 6 and w(B-4) == 30
        print(f'  at {p}: A={A}(vs={va}) B={B}(vs={vb})')

print('\n--- raw scan: (0,13) returns ---')
for p in range(76, n-8, 4):
    if w(p) == 0 and w(p+4) == 13:
        print(f'  at {p}')

print('\n--- raw scan: (0,1,X,Y) ---')
for p in range(76, n-16, 4):
    if w(p) == 0 and w(p+4) == 1:
        print(f'  at {p}: X={w(p+8)} Y={w(p+12)}')