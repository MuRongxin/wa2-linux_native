import struct, zlib
def write_png(fn, pix, w, h):
    def chunk(t, data):
        c = struct.pack('>I',len(data))+t+data
        return c + struct.pack('>I', zlib.crc32(t+data)&0xffffffff)
    raw=b''
    for y in range(h):
        raw += b'\x00'+bytes(pix[y*w:(y+1)*w])
    png=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',w,h,8,0,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b'')
    open(fn,'wb').write(png)
d=open('/tmp/font_jp.tga','rb').read()
idlen=d[0]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
w,h=struct.unpack('<HH', d[12:16])
cb=cm_depth//8
pal_off=18+idlen; px_off=pal_off+cm_len*cb
cols=w//24; rows=h//24
def strip(cy, n=cols):
    pix=[]
    for y in range(24):
        for cx in range(n):
            for x in range(24):
                v=d[px_off+((cy*24+y)*w+(cx*24+x))]
                pix.append(0 if v==0 else 255)
    return pix
write_png('/tmp/fontjp_row0.png', strip(0), cols*24, 24)
write_png('/tmp/fontjp_row1.png', strip(1), cols*24, 24)
print('ok, cells per row', cols)