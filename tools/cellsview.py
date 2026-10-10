import struct
d=open('/tmp/font_jp.tga','rb').read()
idlen,cmtype,imgtype=d[0],d[1],d[2]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
w,h=struct.unpack('<HH', d[12:16])
desc=d[17]
print('desc=0x%02x  bit5(bottom-up)=%d' % (desc, (desc>>5)&1))
pal_off=18+idlen; cb=cm_depth//8
px_off=pal_off+cm_len*cb
cols=w//24
# extract a few cells (as RGB white-on-black for viewing) into one strip PNG (PGM->use ppm)
cells=[0,75,1054,1256,1461,2170]
import sys
out=open('/tmp/cells_view.ppm','wb')
out.write(b'P6\n%d %d\n255\n' % (len(cells)*24, 24))
for cell in cells:
    cx=cell%cols; cy=cell//cols
    for y in range(24):
        for x in range(24):
            v=d[px_off+((cy*24+y)*w+(cx*24+x))]
            # palette alpha as gray level; index0 -> black
            if v==0: out.write(bytes([0,0,0]))
            else:
                a=d[pal_off+v*cb+3]
                out.write(bytes([255,255,255]))
out.close()
print('wrote /tmp/cells_view.ppm for cells', cells)