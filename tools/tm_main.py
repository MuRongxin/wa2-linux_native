import numpy as np, glob, json
from PIL import Image, ImageDraw, ImageFont
from scipy.optimize import linear_sum_assignment
feats=np.load('/tmp/feats_main.npy')
chars=set()
for fn in glob.glob('/tmp/script_out/*.txt'):
    try: chars.update(open(fn,'rb').read().decode('cp932'))
    except: pass
chars=sorted(c for c in chars if ord(c)>0x7f)
N=len(chars); M=feats.shape[0]
fonts=[('sans','/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'),('serif','/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc')]
def nf(b):
    ys,xs=np.where(b>0)
    if len(ys)==0: return np.zeros(256,dtype=np.float32)
    crop=b[ys.min():ys.max()+1, xs.min():xs.max()+1]
    im=Image.fromarray((crop*255).astype(np.uint8)).resize((16,16),Image.BILINEAR)
    v=np.array(im,dtype=np.float32).ravel()/255.0
    return v/(np.linalg.norm(v)+1e-9)
best=np.full((N,M),-1.0,dtype=np.float32)
for nm,path in fonts:
    for size in [30,34,38]:
        f=ImageFont.truetype(path,size)
        Q=np.zeros((N,256),dtype=np.float32)
        for i,ch in enumerate(chars):
            img=Image.new('L',(60,60),0); dr=ImageDraw.Draw(img); dr.text((8,8),ch,font=f,fill=255)
            Q[i]=nf((np.array(img)>110).astype(np.float32))
        sc=feats@Q.T
        best=np.maximum(best, sc.T)
rows,cols=linear_sum_assignment(-best)
mp={}; conf=0
for r_,c_ in zip(rows,cols):
    mp[chars[r_]]=int(c_)
    if best[r_,c_]>0.85: conf+=1
print('assigned',len(mp),'conf>0.85:',conf)
with open('/tmp/jp_map2.txt','w') as f:
    for ch,cell in mp.items(): f.write(ch+'\t'+str(cell)+'\n')
print('wrote /tmp/jp_map2.txt')