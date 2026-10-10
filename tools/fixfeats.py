import struct, numpy as np
from PIL import Image
d=open('/tmp/font_jp.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
W,H=struct.unpack('<HH', d[12:16])
cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
desc=d[17]
flip = (desc & 0x20)==0   # MATCHES the Rust decoder
cols=W//24; rows=H//24; total=cols*rows
palA=[d[pal_off+i*cb+3] for i in range(cm_len)]
print('desc=0x%02x flip=%s'%(desc,flip))
feats=np.zeros((total,256),dtype=np.float32)
for cell in range(total):
    c=cell%cols; r=cell//cols
    prow=(rows-1-r) if flip else r
    b=np.zeros((24,24),dtype=np.float32)
    for y in range(24):
        base=px_off+(prow*24+y)*W+c*24
        for x in range(24):
            idx=d[base+x]
            if idx!=0: b[y,x]=palA[idx]/255.0
    ys,xs=np.where(b>0)
    if len(ys):
        crop=b[ys.min():ys.max()+1, xs.min():xs.max()+1]
        im=Image.fromarray((np.clip(crop,0,1)*255).astype(np.uint8)).resize((16,16),Image.BILINEAR)
        v=np.array(im,dtype=np.float32).ravel()/255.0
        feats[cell]=v/(np.linalg.norm(v)+1e-9)
np.save('/tmp/atlas_feats_fixed.npy',feats)
print('rebuilt feats',feats.shape,'nonblank cells',int((np.linalg.norm(feats,axis=1)>0).sum()))