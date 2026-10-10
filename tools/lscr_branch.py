import struct

def load(name):
    bnr = open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()
    txt = open(f'/home/lrxin/wa2tool/extract/script/{name}.txt', 'rb').read()
    return bnr, txt.split(b',')

bnr, tokens = load('2003')

def dump_at(off, span=32):
    print(f'--- raw u32 at {off} ({span} words) ---')
    for i in range(off, min(off + span*4, len(bnr)), 4):
        v = struct.unpack_from('<I', bnr, i)[0]
        note = ''
        if v == 6: note = '<- record start'
        print(f'  {i:6d}: {v:12d} 0x{v:08X} {note}')

# First char of token text at an op3 reference, for quick identification
def recstart_text(off):
    # decode a few ops from off and return first tok text found
    i = off
    for _ in range(8):
        if i + 8 > len(bnr): break
        op = struct.unpack_from('<I', bnr, i)[0]
        if op == 6:
            i += 8
        elif op == 3:
            n = struct.unpack_from('<I', bnr, i+4)[0]
            s = tokens[n].decode('shift-jis','replace')[:40] if n < len(tokens) else 'oob'
            return f'tok#{n}: {s!r}'
        elif op == 5:
            i += 12
        elif op <= 64:
            i += 8
        else:
            i += 4
    return '(no tok)'

print('=== target A: 36772 ===')
dump_at(36772, 24)
print('first tok near 36772:', recstart_text(36772))
print()
print('=== target B: 39696 ===')
dump_at(39696, 24)
print('first tok near 39696:', recstart_text(39696))
print()

# Also check what's at the offsets after the branch (fall-through, 34132 and later)
# And find which option index (99?) refers to what
print('=== fall-through region 34132..34392 ===')
for i in range(34132, 34392, 4):
    v = struct.unpack_from('<I', bnr, i)[0]
    print(f'  {i:6d}: {v:12d} 0x{v:08X}')