import struct
d=open('/tmp/font_jp.tga','rb').read()
idlen,cmtype,imgtype=d[0],d[1],d[2]
cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
w,h=struct.unpack('<HH', d[12:16])
pal_off=18+idlen; cb=cm_depth//8
px_off=pal_off+cm_len*cb
cols=w//24
def show(cell,label):
    cx=cell%cols; cy=cell//cols
    print('cell',cell,label)
    for y in range(24):
        row=''
        for x in range(24):
            v=d[px_off+((cy*24+y)*w+(cx*24+x))]
            row+='#' if v!=0 else '.'
        print(' ',row)
show(0,'(cp932 0x889F -> should be 一 per code_maps)')
show(75,'(cp932 0x88EA = 一 actual)')