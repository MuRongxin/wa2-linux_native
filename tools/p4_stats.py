import struct, glob, os
from collections import defaultdict

# char.pak file counts per prefix
path = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak'
f = open(path, 'rb')
n = struct.unpack_from('<I', f.read(16), 12)[0]
from collections import Counter
prefix_count = Counter()
prefix_names = defaultdict(list)
for i in range(n):
    f.seek(16 + i*44)
    e = f.read(44)
    name = e[4:28].split(b'\x00')[0].decode('shift-jis', 'replace')
    prefix_count[name[:3]] += 1
    prefix_names[name[:3]].append(name)

# collect p4 (5th param) stats per char id
p4s = defaultdict(set)
for bp in sorted(glob.glob('/home/lrxin/wa2tool/extract/script/*.bnr')):
    bnr = open(bp, 'rb').read()
    nn = len(bnr)
    def w(off): return struct.unpack_from('<I', bnr, off)[0]
    i = 28
    vals = []
    while i + 12 <= nn:
        op = w(i)
        if op == 6:
            vals = []; i += 8
        elif op == 5:
            vals.append(w(i+8)); i += 12
        elif op == 4:
            if w(i+4) == 138 and len(vals) >= 5:
                p0, p4 = vals[-5], vals[-1]
                p4s[p0].add(p4)
            i += 8
        elif op in (3,):
            i += 8
        elif op == 0:
            sub = w(i+4)
            i += 12 if sub == 12 else (16 if sub in (1,2) else 8)
        elif op in (1, 2):
            i += 8
        else:
            i += 4

names = {0:'春希',1:'かずさ',2:'雪菜',3:'小春',4:'千晶',5:'麻理',10:'武也',11:'依緒',
         14:'雪菜の母',15:'孝宏',16:'曜子',19:'朋',20:'佐藤',23:'早百合',24:'亜子',
         25:'美穂子',27:'上原',28:'和希',30:'浜田',31:'松岡',33:'鈴木',34:'佐和子',35:'美代子'}
prefix_guess = {0:'har?',1:'kaz',2:'set',3:'koh',4:'izu',5:'mar',10:'tak',11:'ioo',
                15:'?',16:'you',19:'tom',20:'sat',23:'say',24:'aco',25:'mih',
                27:'ueh',28:'kiz',30:'ham',31:'mat',33:'suz',34:'saw',35:'miy'}
print(f'{"id":>3} {"name":<10} {"prefix":<6} {"files":>6} {"p4_distinct":>11} {"p4_max":>7}')
for cid in sorted(p4s):
    s = p4s[cid]
    pf = prefix_guess.get(cid, '?')
    cnt = prefix_count.get(pf, 0)
    print(f'{cid:>3} {names.get(cid,"?"):<10} {pf:<6} {cnt:>6} {len(s):>11} {max(s):>7}')