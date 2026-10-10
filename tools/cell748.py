import struct
def art(fn, cell, cols=80, csize=24):
    d=open(fn,'rb').read()
    idlen=d[0]
    cm_first,cm_len,cm_depth=struct.unpack('<HHB', d[3:8])
    W,H=struct.unpack('<HH', d[12:16])
    cb=cm_depth//8; pal_off=18+idlen; px_off=pal_off+cm_len*cb
    desc=d[17]; flip=(desc&0x20)==0
    rows=H//csize
    c=cell%cols; r=cell//cols; prow=(rows-1-r) if flip else r
    out=[]
    for y in range(24):
        line=''
        for x in range(24):
            idx=d[px_off+(prow*24+y)*W+c*24+x]
            if idx==0: line+='.'
            else:
                rgb=(d[pal_off+idx*cb+2],d[pal_off+idx*cb+1],d[pal_off+idx*cb])
                line+='#' if max(rgb)>100 else '-'
        out.append(line)
    return out
s=open('/tmp/wa2_cset.txt',encoding='utf-8',errors='ignore').read()
nz=[x for x in s if not x.isspace()]
print('wa2_cset[0..9] =', ''.join(nz[:10]))
for fn in ['/tmp/wa2fnt_2.tga','/tmp/wa2fnt_3.tga','/tmp/font_cn.tga']:
    print('---', fn, 'cell 748 ---')
    for l in art(fn,748): print('  ',l)
print('--- wa2fnt_2 cell 0 ---')
for l in art('/tmp/wa2fnt_2.tga',0): print('  ',l)