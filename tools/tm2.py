import struct, numpy as np, glob
from PIL import Image, ImageDraw, ImageFont
d=open('/tmp/font_jp.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
W,H=struct.unpack('<HH', d[12:16])
cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
desc=d[17]; botup=(desc>>5)&1
cols=W//24; rows=H//24; total=cols*rows
palA=[d[pal_off+i*cb+3] for i in range(cm_len)]
def norm_feat(binary24):
    ys,xs=np.where(binary24>0)
    if len(ys)==0: return np.zeros(256,dtype=np.float32)
    crop=binary24[ys.min():ys.max()+1, xs.min():xs.max()+1]
    img=Image.fromarray((crop*255).astype(np.uint8)).resize((16,16), Image.BILINEAR)
    v=np.array(img,dtype=np.float32).ravel()/255.0
    return v/(np.linalg.norm(v)+1e-9)
feats=np.zeros((total,256),dtype=np.float32)
for cell in range(total):
    c=cell%cols; r=cell//cols
    prow=(rows-1-r) if botup else r
    b=np.zeros((24,24),dtype=np.float32)
    for y in range(24):
        base=px_off+(prow*24+y)*W+c*24
        for x in range(24):
            idx=d[base+x]
            if idx!=0: b[y,x]=1
    feats[cell]=norm_feat(b)
np.save('/tmp/atlas_feats.npy',feats)
print('feats',feats.shape)
fonts=[('sans','/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc',0),('serif','/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc',0)]
def render_feat(ch,path,idx,size):
    f=ImageFont.truetype(path,size,index=idx)
    img=Image.new('L',(48,48),0)
    dr=ImageDraw.Draw(img)
    dr.text((6,6),ch,font=f,fill=255)
    a=(np.array(img)>100).astype(np.float32)
    return norm_feat(a)
def best(ch):
    bb=(-1,-2.0)
    for nm,path,idx in fonts:
        for size in [30,34,38]:
            q=render_feat(ch,path,idx,size)
            sc=feats@q
            m=int(np.argmax(sc))
            if sc[m]>bb[1]: bb=(m,float(sc[m]))
    return bb
import sys
for ch in ['日','一','井','人','大','国','手']:
    c,s=best(ch)
    b=ch.encode('cp932')
    print(ch, ch.encode('cp932').hex(), '-> cell', c, 'score', round(s,3))