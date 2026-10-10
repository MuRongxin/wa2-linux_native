import struct
for fn in ['/tmp/font_cn.tga','/tmp/font_jp.tga']:
    d=open(fn,'rb').read()
    idlen,cmtype,imgtype=d[0],d[1],d[2]
    w,h=struct.unpack('<HH', d[12:16])
    depth,desc=d[16],d[17]
    print(fn, 'imgtype=%d %dx%d depth=%d desc=0x%02x bottomup=%d'%(imgtype,w,h,depth,desc,(desc>>5)&1))