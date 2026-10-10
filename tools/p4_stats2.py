import struct, glob, os
from collections import defaultdict

def walk(bnr):
    """Faithful port of the Rust lscr arity table."""
    n = len(bnr)
    def w(off): return struct.unpack_from('<I', bnr, off)[0]
    count = w(8)
    entry = 28
    for k in range(count):
        b = 12 + k*8
        if w(b) == 0:
            entry = w(b+4)
            break
    i = entry
    ops = []
    while i + 4 <= n:
        op = w(i)
        if op == 6:
            ops.append(('rec', w(i+4))); i += 8
        elif op == 5:
            t = w(i+4); v = w(i+8)
            ops.append(('flt' if t == 4 else 'int', v)); i += 12
        elif op == 3:
            ops.append(('tok', w(i+4))); i += 8
        elif op == 4:
            ops.append(('act', w(i+4))); i += 8
        elif op == 0:
            sub = w(i+4)
            if sub == 12: ops.append(('goto', w(i+8))); i += 12
            elif sub == 2: ops.append(('br', w(i+8), w(i+12))); i += 16
            elif sub == 1: ops.append(('ret',)); i += 8
            elif sub == 4: ops.append(('case',)); i += 8
            elif sub == 13: ops.append(('bend',)); i += 8
            elif sub in (5, 14):
                j = i + 8; tg = []
                while j + 4 <= n and len(tg) < 8:
                    v = w(j)
                    if 64 < v < n: tg.append(v); j += 4
                    else: break
                ops.append(('sw', tg)); i = j
            else:
                ops.append(('raw0', sub)); i += 8
        elif op in (1, 2):
            ops.append(('v' if op == 1 else 'f', w(i+4))); i += 8
        else:
            ops.append(('raw', op)); i += 4
    return ops

p4_by_char = defaultdict(set)
samples = defaultdict(list)
for bp in sorted(glob.glob('/home/lrxin/wa2tool/extract/script/*.bnr')):
    bnr = open(bp, 'rb').read()
    ops = walk(bnr)
    vals = []
    cur_speaker = None
    for o in ops:
        if o[0] == 'rec':
            vals = []
        elif o[0] == 'int':
            vals.append(o[1])
        elif o[0] == 'tok':
            pass
        elif o[0] == 'act':
            if o[1] == 138 and len(vals) >= 5:
                p0, p4 = vals[-5], vals[-1]
                p4_by_char[p0].add(p4)
                if len(samples[p0]) < 8:
                    samples[p0].append(tuple(vals[-5:]))
            vals2 = vals
    # speaker-less; fine

names = {0:'春希',1:'かずさ',2:'雪菜',3:'小春',4:'千晶',5:'麻理',10:'武也',11:'依緒',15:'孝宏',16:'曜子',19:'朋',20:'佐藤',23:'早百合',24:'亜子',25:'美穂子',27:'上原',28:'和希',30:'浜田',31:'松岡',33:'鈴木',34:'佐和子',35:'美代子',14:'雪菜の母'}
print(f'{"id":>3} {"name":<10} {"p4_distinct":>11} {"p4_max":>7}  samples')
for cid in sorted(p4_by_char):
    s = p4_by_char[cid]
    print(f'{cid:>3} {names.get(cid,"?"):<10} {len(s):>11} {max(s):>7}  {samples[cid][:5]}')