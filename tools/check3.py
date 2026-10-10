import struct
d=open('/tmp/scene_1001_realbg.tga','rb').read()
w,h=struct.unpack('<HH', d[12:16]); off=18+d[0]
box_y0 = h - h//4 - 24
white=0; purple=0
for y in range(box_y0+10, h-30):
    for x in range(40, w-40):
        i=off+(y*w+x)*4
        b,g,r=d[i],d[i+1],d[i+2]
        if r>180 and g>180 and b>180: white+=1
        elif g<70 and r>50 and b>50: purple+=1
print('scene', w,'x',h)
print('white kanji glyph px:', white)
print('purple fallback px :', purple)