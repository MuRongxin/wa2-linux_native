import struct
d=open('/tmp/kanji_test.tga','rb').read()
w,h=struct.unpack('<HH', d[12:16]); off=18+d[0]
box_y0 = h - h//4 - 24
white=0; purple=0; minx=9999; maxx=-1; miny=9999; maxy=-1
for y in range(box_y0, h):
    for x in range(0, w):
        i=off+(y*w+x)*4
        b,g,r=d[i],d[i+1],d[i+2]
        if r>180 and g>180 and b>180:
            white+=1
            if x<minx: minx=x
            if x>maxx: maxx=x
            if y<miny: miny=y
            if y>maxy: maxy=y
        elif g<70 and r>50 and b>50: purple+=1
print('white(kanji) px:', white, ' purple px:', purple)
print('white bbox x:[%d..%d] y:[%d..%d]'%(minx,maxx,miny,maxy))