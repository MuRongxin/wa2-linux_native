import struct
def load_tga(fn):
    d=open(fn,'rb').read()
    idlen=d[0]; imgtype=d[2]
    cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
    w,h,pxdepth=struct.unpack('<HHH', d[12:18])
    off=18+idlen
    if cm_len: off+=cm_len*(cm_depth//8)
    px=d[off:off+w*h]
    return w,h,px
w,h,px=load_tga('/tmp/font_jp.tga')
cols=w//24
def nb(cx,cy):
    c=0
    for y in range(cy*24,cy*24+24):
        for x in range(cx*24,cx*24+24):
            if px[y*w+x]!=0: c+=1
    return c
# scan blocks of 80 cells, report count of non-blank cells
res=[]
for start in range(0, 7680, 80):
    cnt=sum(1 for i in range(start,start+80) if nb(i%cols,i//cols)>5)
    res.append((start,cnt))
print('block_start : #nonblank_cells (of 80)')
for s,c in res:
    if c>0: print(s, c)