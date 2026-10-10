import struct
def load(fn):
    d=open(fn,'rb').read()
    idlen=d[0]; cmtype=d[1]
    cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
    w,h=struct.unpack('<HH', d[12:16])
    depth,desc=d[16],d[17]
    off=18+idlen
    if cm_len: off+=cm_len*(cm_depth//8)
    return w,h,d,off,(depth+7)//8,(desc>>5)&1
def cell_gray(fn,x0,y0):
    w,h,d,off,pb,botup=load(fn)
    rows=[]
    for y in range(24):
        row=''
        for x in range(24):
            i=off+((y0+y)*w+(x0+x))*pb
            lum=d[i+2] if pb==4 else d[i]
            row+='#' if lum>128 else '.'
        rows.append(row)
    return rows
print('=== atlas cell 748 (tga-dump x=672,y=216) ===')
for r in cell_gray('/tmp/font_cn_mine.tga',672,216): print(r)
print('=== cn_test glyph0 (x=60,y=520) ===')
for r in cell_gray('/tmp/cn_test.tga',60,520): print(r)