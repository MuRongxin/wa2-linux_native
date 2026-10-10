import struct
d=open('/tmp/font_cn.tga','rb').read()
idlen,cmtype,imgtype=d[0],d[1],d[2]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
w,h=struct.unpack('<HH', d[12:16])
pal_off=18+idlen; cb=cm_depth//8
px_off=pal_off+cm_len*cb
print('font_cn palette first 8 (B,G,R,A):')
for i in range(8):
    e=pal_off+i*cb
    print('  ',i, tuple(d[e:e+cb]))
cols=w//24
cell=748
cx=cell%cols; cy=cell//cols
hist={}
for y in range(24):
    for x in range(24):
        v=d[px_off+((cy*24+y)*w+(cx*24+x))]
        hist[v]=hist.get(v,0)+1
print('cell748 histogram:', dict(sorted(hist.items())))