import struct
d=open('/tmp/font_jp.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
w,h=struct.unpack('<HH', d[12:16])
desc=d[17]; botup=(desc>>5)&1
cb=cm_depth//8
pal_off=18+idlen; px_off=pal_off+cm_len*cb
cols=w//24; rows=h//24
# logical row 0 -> physical row (bottom-up aware)
prow = (rows-1-0) if botup else 0
out=open('/tmp/row0.pgm','wb')
out.write(('P5\n%d %d\n255\n'%(cols*24,24)).encode())
for y in range(24):
    for cx in range(cols):
        v=d[px_off+((prow*24+y)*w+(cx*24))]
        out.write(bytes([0 if v==0 else 255]))
out.close()
print('wrote row0 (cells 0..%d)'%(cols-1))