import struct
def analyze(fn):
    d=open(fn,'rb').read()
    idlen=d[0]
    cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
    W,H=struct.unpack('<HH', d[12:16])
    cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
    cols=W//24; rows=H//24; total=cols*rows
    desc=d[17]; botup=(desc>>5)&1
    def nb(cell):
        if cell<0 or cell>=total: return -1
        c=cell%cols; r=cell//cols; prow=(rows-1-r) if botup else r
        n=0
        for y in range(24):
            base=px_off+(prow*24+y)*W+c*24
            for x in range(24):
                if d[base+x]!=0: n+=1
        return n
    print(fn)
    for cell in [0,1,75,100,300,500,747,748,749,1000,3832,3833,5000,7679]:
        print('   cell',cell,'nonblank',nb(cell))
analyze('/tmp/font_cn.tga')
analyze('/tmp/font_jp.tga')