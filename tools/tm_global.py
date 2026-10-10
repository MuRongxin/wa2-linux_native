import numpy as np, glob, struct
from PIL import Image, ImageDraw, ImageFont
from scipy.optimize import linear_sum_assignment
feats=np.load('/tmp/atlas_feats.npy')
chars=set()
for fn in glob.glob('/tmp/script_out/*.txt'):
    try: chars.update(open(fn,'rb').read().decode('cp932'))
    except: pass
chars=sorted(c for c in chars if ord(c)>0x7f)
N=len(chars); M=feats.shape[0]
fonts=[('sans','/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'),('serif','/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc')]
def norm_feat(binary):
    ys,xs=np.where(binary>0)
    if len(ys)==0: return np.zeros(256,dtype=np.float32)
    crop=binary[ys.min():ys.max()+1, xs.min():xs.max()+1]
    img=Image.fromarray((crop*255).astype(np.uint8)).resize((16,16), Image.BILINEAR)
    v=np.array(img,dtype=np.float32).ravel()/255.0
    return v/(np.linalg.norm(v)+1e-9)
best=np.full((N,M),-1.0,dtype=np.float32)
for nm,path in fonts:
    for size in [28,32,36]:
        f=ImageFont.truetype(path,size)
        Q=np.zeros((N,256),dtype=np.float32)
        for i,ch in enumerate(chars):
            img=Image.new('L',(56,56),0); dr=ImageDraw.Draw(img)
            dr.text((8,8),ch,font=f,fill=255)
            Q[i]=norm_feat((np.array(img)>100).astype(np.float32))
        sc=feats@Q.T  # (M,N)
        best=np.maximum(best, sc.T)  # (N,M)
np.save('/tmp/score_best.npy', best)
print('score matrix', best.shape)
rows,cols=linear_sum_assignment(-best)
mapping={}
conf=0
for r,c in zip(rows,cols):
    mapping[chars[r]]=int(c)
    if best[r,c]>0.75: conf+=1
print('assigned', len(mapping), 'high-conf(>0.75)', conf)
import json
json.dump({k:v for k,v in mapping.items()}, open('/tmp/jp_map.json','w'))
for ch in ['日','一','井','人','大','国','手','月','火','水']:
    if ch in mapping:
        print(ch, mapping[ch], round(float(best[chars.index(ch), mapping[ch]]),3))