import struct
def load_tga(fn):
    d=open(fn,'rb').read()
    idlen=d[0]; cmtype=d[1]; imgtype=d[2]
    cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
    w,h,pxdepth=struct.unpack('<HHH', d[12:18])
    off=18+idlen
    if cm_len: off+=cm_len*(cm_depth//8)
    px=d[off:off+w*h]
    return w,h,px
def cell_nonblank(px,w,cx,cy,cell=24):
    cnt=0
    for y in range(cy*cell, cy*cell+cell):
        for x in range(cx*cell, cx*cell+cell):
            if px[y*w+x]!=0: cnt+=1
    return cnt
for name in ['/tmp/font_jp.tga','/tmp/font_cn.tga']:
    w,h,px=load_tga(name)
    cols=w//24; rows=h//24
    print(name, 'dims', w,h, 'cells', cols,'x',rows, 'total', cols*rows)
    print('  cell0 nonblank:', cell_nonblank(px,w,0,0))
    print('  cell748 nonblank:', cell_nonblank(px,w,748%cols,748//cols))
    print('  cell747 nonblank:', cell_nonblank(px,w,747%cols,747//cols))
    print('  cell3832 nonblank:', cell_nonblank(px,w,3832%cols,3832//cols))