import struct
w,h=1280,720
hdr=bytes([0,0,2,0,0,0,0,0,0,0,0,0])+struct.pack('<HH',w,h)+bytes([32,0x28])
rows=bytearray()
for y in range(h):
    for x in range(w):
        r=int(20+80*x/w); g=int(30+40*y/h); b=int(60+100*(1-y/h))
        rows+=bytes([b,g,r,255])
open('/tmp/bg_gradient.tga','wb').write(hdr+bytes(rows))
print('bg written', 18+len(rows))