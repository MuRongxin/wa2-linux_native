import struct, numpy as np
from PIL import Image
d=open('/tmp/wa2fnt_3.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
W,H=struct.unpack('<HH', d[12:16])
cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
desc=d[17]; flip=(desc&0x20)==0
cols=W//24; rows=H//24; total=cols*rows
print('atlas',W,H,'flip',flip,'cells',total)
feats=np.zeros((total,256),dtype=np.float32)
for cell in range(total):
    c=cell%cols; r=cell//cols; prow=(rows-1-r) if flip else r
    b=np.zeros((24,24),dtype=np.float32)
    for y in range(24):
        base=px_off+(prow*24+y)*W+c*24
        for x in range(24):
            if d[base+x]!=0: b[y,x]=1.0
    ys,xs=np.where(b>0)
    if len(ys):
        crop=b[ys.min():ys.max()+1, xs.min():xs.max()+1]
        im=Image.fromarray((crop*255).astype(np.uint8)).resize((16,16),Image.BILINEAR)
        v=np.array(im,dtype=np.float32).ravel()/255.0
        feats[cell]=v/(np.linalg.norm(v)+1e-9)
np.save('/tmp/feats_main.npy',feats)
print('feats built from MAIN atlas')