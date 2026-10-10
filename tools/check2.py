import struct
d=open('/tmp/scene_1001.tga','rb').read()
w,h=struct.unpack('<HH', d[12:16])
off=18+d[0]
box_y0 = h - h//4 - 24
white=0; purple=0
for y in range(box_y0+10, h-30):
    for x in range(40, w-40):
        i=off+(y*w+x)*4
        b,g,r=d[i],d[i+1],d[i+2]
        if r>180 and g>180 and b>180: white+=1
        elif g<70 and r>50 and b>50 and not(r>180 and g>180): purple+=1
print('white (kanji glyphs):', white)
print('purple (fallback boxes, kana/ascii):', purple)