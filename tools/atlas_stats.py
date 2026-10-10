import struct
def stats(fn):
    d=open(fn,'rb').read()
    idlen=d[0]
    cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
    W,H=struct.unpack('<HH', d[12:16])
    cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
    desc=d[17]; flip=(desc&0x20)==0
    cols=W//24; rows=H//24
    nz=0
    for cell in range(cols*rows):
        c=cell%cols; r=cell//cols; prow=(rows-1-r) if flip else r
        cnt=0
        for y in range(24):
            base=px_off+(prow*24+y)*W+c*24
            for x in range(24):
                if d[base+x]!=0: cnt+=1
        if cnt>0: nz+=1
    print('%s: %dx%d depth=%d desc=0x%02x cols=%d rows=%d cells=%d nonblank=%d' % (fn,W,H,d[16],desc,cols,rows,cols*rows,nz))
for f in ['/tmp/font_cn.tga','/tmp/wa2fnt_2.tga','/tmp/wa2fnt_3.tga']:
    try: stats(f)
    except Exception as e: print(f,'err',e)