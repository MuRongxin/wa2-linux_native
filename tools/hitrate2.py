import struct, glob
d=open('/tmp/font_jp.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
W,H=struct.unpack('<HH', d[12:16])
cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
cols=W//24; rows=H//24; total=cols*rows
desc=d[17]; botup=(desc>>5)&1
def nb(cell):
    if cell<0 or cell>=total: return -1
    cx=cell%cols; cy=cell//cols
    prow=(rows-1-cy) if botup else cy
    n=0
    for y in range(24):
        base=px_off+((prow*24+y)*W)+cx*24
        for x in range(24):
            if d[base+x]!=0: n+=1
    return n
chars=set()
for fn in glob.glob('/tmp/script_out/*.txt'):
    try: chars.update(open(fn,'rb').read().decode('cp932'))
    except: pass
chars={c for c in chars if ord(c)>0x7f}
res={'hit':0,'blank':0,'oob':0,'lead_other':0}
examples=[]
for c in sorted(chars):
    b=c.encode('cp932')
    if len(b)!=2: continue
    if not (0x88<=b[0]<=0x9c): res['lead_other']+=1; continue
    cell=189*(b[0]-0x88)+(b[1]-0x40)-95
    v=nb(cell)
    if v<0: res['oob']+=1
    elif v>0: res['hit']+=1
    else: res['blank']+=1; examples.append(c)
print('formula lead 0x88-0x9c ->', res)
print('blank examples (first 20):', ''.join(examples[:20]))