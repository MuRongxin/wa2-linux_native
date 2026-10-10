import struct
d=open('/tmp/font_jp.tga','rb').read()
idlen,cmtype,imgtype=d[0],d[1],d[2]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
print('imgtype=%d cm_first=%d cm_len=%d cm_depth=%d' % (imgtype,cm_first,cm_len,cm_depth))
pal_off=18+idlen
cb=cm_depth//8
print('first 6 palette entries:')
for i in range(6):
    e=pal_off+i*cb
    print('  idx',i,':',tuple(d[e:e+cb]))
w,h=struct.unpack('<HH', d[12:16])
px_off=pal_off+cm_len*cb
hist={}
for y in range(0,24):
    for x in range(0,24):
        v=d[px_off+(y*w+x)]
        hist[v]=hist.get(v,0)+1
print('cell0 index histogram:', dict(sorted(hist.items())))