import struct
d=open('/tmp/jpatlas.tga','rb').read()
W,H=struct.unpack('<HH', d[12:16]); off=18+d[0]
print('decoded atlas', W,'x',H)
# count pixels with A>0 per 24px row-band (logical rows 0..95)
cnts=[]
for r in range(96):
    n=0
    for y in range(r*24,(r+1)*24):
        base=off+y*W*4
        for x in range(W):
            if d[base+x*4+3]>0: n+=1
    cnts.append(n)
print('rows with content (first 20):', [(i,c) for i,c in enumerate(cnts) if c>0][:20])
print('empty rows:', sum(1 for c in cnts if c==0), 'of 96')
print('total content rows:', sum(1 for c in cnts if c>0))