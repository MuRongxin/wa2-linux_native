import struct
def load(fn):
    d=open(fn,'rb').read()
    w,h=struct.unpack('<HH', d[12:16])
    off=18+d[0]
    return w,h,d,off
w,h,d,off=load('/tmp/scene_1001.tga')
print('scene', w,'x',h, 'expect 1280x720')
# dialog box region: bottom quarter. Count white (kanji glyph) vs magenta (fallback) pixels.
box_y0 = h - h//4 - 24
white=0; magenta=0; total=0
for y in range(box_y0+10, h-30):
    for x in range(40, w-40):
        i=off+(y*w+x)*4
        b,g,r,a=d[i],d[i+1],d[i+2],d[i+3]
        if a>0:
            total+=1
            if r>200 and g>200 and b>200: white+=1
            elif r>150 and b>150 and g<100: magenta+=1
print('white glyph px (kanji):', white)
print('magenta fallback px (kana/ascii):', magenta)