import struct
def load(fn):
    d=open(fn,'rb').read()
    idlen,cmtype,imgtype=d[0],d[1],d[2]
    cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
    w,h=struct.unpack('<HH', d[12:16])
    off=18+idlen
    if cm_len: off+=cm_len*(cm_depth//8)
    return w,h,d,off
w,h,d,off=load('/tmp/font_jp.tga')
cols=w//24
def cellnb(cell):
    cx=cell%cols; cy=cell//cols
    c=0
    for y in range(cy*24,cy*24+24):
        for x in range(cx*24,cx*24+24):
            if d[off+(y*w+x)]!=0: c+=1
    return c
test='\u4e00\u4e8c\u4e09\u5341\u4eba\u5927\u65e5\u6708\u5c71\u5ddd\u7530\u53e3\u76ee'
print('char cp932  lead trail  formula_cell  nonblank_in_that_cell')
for ch in test:
    b=ch.encode('cp932')
    lead,trail=b[0],b[1]
    cell=189*(lead-0x88)+(trail-0x40)-95
    nb=cellnb(cell) if cell>=0 else -1
    print(f'  {ch}   {b.hex()}   {lead:02X} {trail:02X}    {cell:5d}       {nb}')