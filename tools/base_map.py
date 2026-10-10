import struct, numpy as np, glob
from PIL import Image, ImageDraw, ImageFont
from scipy.optimize import linear_sum_assignment
# CN atlas (font_cn) cells 0..747 = preserved base glyphs (kana/ASCII/symbols)
d=open('/tmp/font_cn.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
W,H=struct.unpack('<HH', d[12:16])
cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
desc=d[17]; flip=(desc&0x20)==0
cols=W//24; rows=H//24
BASE=748
def cellbin(cell):
    c=cell%cols; r=cell//cols; prow=(rows-1-r) if flip else r
    b=np.zeros((24,24),dtype=np.float32)
    for y in range(24):
        base=px_off+(prow*24+y)*W+c*24
        for x in range(24):
            idx=d[base+x]
            if idx!=0:
                rgb=(d[pal_off+idx*cb+2],d[pal_off+idx*cb+1],d[pal_off+idx*cb])
                b[y,x]=1.0 if max(rgb)>100 else 0.0
    return b
def nf(b):
    ys,xs=np.where(b>0)
    if len(ys)==0: return None
    crop=b[ys.min():ys.max()+1, xs.min():xs.max()+1]
    im=Image.fromarray((crop*255).astype(np.uint8)).resize((16,16),Image.BILINEAR)
    v=np.array(im,dtype=np.float32).ravel()/255.0
    n=np.linalg.norm(v)
    return v/n if n>1e-9 else None
feats=[nf(cellbin(i)) for i in range(BASE)]
print('base cells with glyphs:', sum(1 for f in feats if f is not None))
# candidate chars: kana + ascii-printable + common punctuation
cands=[]
for cp in list(range(0x3041,0x3097))+list(range(0x30a1,0x30fb))+list(range(0x20,0x7f))+[0x3001,0x3002,0x300c,0x300d,0x300e,0x300f,0x30fb,0x30fc,0x2015,0x2026]:
    cands.append(chr(cp))
cands=sorted(set(cands))
print('candidates:', len(cands))
fonts=[('sans','/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'),('serif','/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc')]
def qf(ch):
    best=None; bs=0
    for nm,path in fonts:
        for size in [30,34,38]:
            f=ImageFont.truetype(path,size)
            img=Image.new('L',(60,60),0); dr=ImageDraw.Draw(img); dr.text((8,8),ch,font=f,fill=255)
            q=nf((np.array(img)>110).astype(np.float32))
            if q is None: continue
            sc=max(float(fq@q) for fq in feats if fq is not None)
            if sc>bs: bs=sc; best=(q,sc)
    return best
# build score matrix candidates x base cells
S=np.zeros((len(cands),BASE),dtype=np.float32)
for i,ch in enumerate(cands):
    q=None; bs=-1
    for nm,path in fonts:
        for size in [30,34,38]:
            f=ImageFont.truetype(path,size)
            img=Image.new('L',(60,60),0); dr=ImageDraw.Draw(img); dr.text((8,8),ch,font=f,fill=255)
            qq=nf((np.array(img)>110).astype(np.float32))
            if qq is None: continue
            for j in range(BASE):
                if feats[j] is None: continue
                v=float(feats[j]@qq)
                if v>S[i,j]: S[i,j]=v
rows,cols_=linear_sum_assignment(-S)
mp={}
conf=0
for r_,c_ in zip(rows,cols_):
    mp[cands[r_]]=int(c_)
    if S[r_,c_]>0.85: conf+=1
print('kana/ascii assigned:',len(mp),'conf>0.85:',conf)
with open('/tmp/base_map.txt','w') as f:
    for ch,cell in mp.items(): f.write(ch+'\t'+str(cell)+'\n')
print('wrote /tmp/base_map.txt')