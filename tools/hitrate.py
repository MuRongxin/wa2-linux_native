import struct, glob, os
d=open('/tmp/font_jp.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
W,H=struct.unpack('<HH', d[12:16])
cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
cols=W//24; rows=H//24
desc=d[17]; botup=(desc>>5)&1
def cell_nonblank(cell):
    cx=cell%cols; cy=cell//cols
    prow=(rows-1-cy) if botup else cy
    n=0
    for y in range(24):
        base=(prow*24+y)*W
        for x in range(24):
            if d[px_off+base+cx*24+x]!=0: n+=1
    return n
# gather unique chars from all scenario txt files
chars=set()
for fn in glob.glob('/tmp/script_out/*.txt'):
    try: s=open(fn,'rb').read().decode('cp932')
    except: continue
    chars.update(s)
chars={c for c in chars if ord(c)>0x7f}
print('unique non-ascii chars in scripts:', len(chars))
kanji=[]; kana=[]; other=[]
for c in chars:
    b=c.encode('cp932')
    if len(b)==2:
        (kanji if b[0]>=0x88 else kana).append(c)
    else: other.append(c)
print('2-byte lead>=0x88 (kanji-ish):', len(kanji), ' lead<0x88 (kana/symbols):', len(kana), ' 1-byte:', len(other))
hit=0; miss=0; blank=0
for c in kanji:
    b=c.encode('cp932')
    cell=189*(b[0]-0x88)+(b[1]-0x40)-95
    if cell<0: miss+=1; continue
    nb=cell_nonblank(cell)
    if nb>0: hit+=1
    else: blank+=1
print('kanji formula cells: hit(nonblank)=%d blank=%d neg=%d'%(hit,blank,miss))