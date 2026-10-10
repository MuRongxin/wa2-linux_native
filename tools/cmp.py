import struct
def load_tga(fn):
    d=open(fn,'rb').read()
    idlen,cmtype,imgtype=d[0],d[1],d[2]
    cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
    w,h=struct.unpack('<HH', d[12:16])
    depth,desc=d[16],d[17]
    off=18+idlen
    if cm_len: off+=cm_len*(cm_depth//8)
    pb=(depth+7)//8
    flip = (desc & 0x20)==0
    return w,h,d,off,pb,flip
def rgba_at(fn, x, y):
    w,h,d,off,pb,flip=load_tga(fn)
    row = (h-1-y) if flip else y
    i=off+(row*w+x)*pb
    if pb==3:  # BGR
        return (d[i+2], d[i+1], d[i], 255)
    elif pb==4:  # BGRA (my save format)
        return (d[i+2], d[i+1], d[i], d[i+3])
# sample points: (skin, dark hair, couch, bg wall) in the 1280x720 image
pts=[(300,400),(760,120),(100,600),(1150,400),(640,360)]
orig='/tmp/grp_out/v208100.tga'
mine='/tmp/scene_1001_realbg.tga'
print('point        original(BGR->RGB)   my-scene-render')
for (x,y) in pts:
    o=rgba_at(orig,x,y); m=rgba_at(mine,x,y)
    print(f'({x},{y})  {str(o):18s}  {str(m)}')