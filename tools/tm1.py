import struct, numpy as np
from PIL import Image, ImageDraw, ImageFont
d=open('/tmp/font_jp.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
W,H=struct.unpack('<HH', d[12:16])
cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
desc=d[17]; botup=(desc>>5)&1
cols=W//24; rows=H//24; total=cols*rows
palA=[d[pal_off+i*cb+3] for i in range(cm_len)]
# build logical-cell matrix (matching my decoder's flip) : logical row r <- source row (rows-1-r)
cells=np.zeros((total,576),dtype=np.float32)
for cell in range(total):
    c=cell%cols; r=cell//cols
    prow=(rows-1-r) if botup else r
    v=0
    for y in range(24):
        base=px_off+(prow*24+y)*W+c*24
        for x in range(24):
            idx=d[base+x]
            if idx!=0: cells[cell,y*24+x]=palA[idx]/255.0
print('atlas matrix', cells.shape, 'nonzero frac', (cells>0).mean())
np.save('/tmp/atlas_cells.npy', cells)
def render(ch, size):
    f=ImageFont.truetype('/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc', size)
    img=Image.new('L',(24,24),0)
    dr=ImageDraw.Draw(img)
    bbox=dr.textbbox((0,0),ch,font=f)
    w=bbox[2]-bbox[0]; h=bbox[3]-bbox[1]
    x=(24-w)//2-bbox[0]; y=(24-h)//2-bbox[1]
    dr.text((x,y),ch,font=f,fill=255)
    a=np.array(img,dtype=np.float32)/255.0
    return a
def match(ch, size):
    q=render(ch,size).ravel()
    qn=q/ (np.linalg.norm(q)+1e-9)
    cn=cells/ (np.linalg.norm(cells,axis=1,keepdims=True)+1e-9)
    scores=cn@qn
    best=np.argsort(-scores)[:3]
    return [(int(b),float(scores[b])) for b in best]
for ch in ['日','一','井','人','大']:
    for size in [20,22]:
        m=match(ch,size)
        print(ch, 'size',size, '->', [(c,round(s,3)) for c,s in m])