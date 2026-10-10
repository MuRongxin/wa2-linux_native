import struct
d=open('/tmp/jpatlas.tga','rb').read()
W,H=struct.unpack('<HH', d[12:16]); off=18+d[0]
cnts=[]
for r in range(96):
    n=0
    for y in range(r*24,(r+1)*24):
        base=off+y*W*4
        for x in range(W):
            if d[base+x*4+2]>128: n+=1   # R channel (tga-dump writes BGRA; so +2 = R)
    cnts.append(n)
rows=[(i,c) for i,c in enumerate(cnts) if c>0]
print('decoded rows with white pixels:', len(rows), 'of 96')
print('first 15:', rows[:15])
print('last 5:', rows[-5:])
print('empty row indices:', [i for i,c in enumerate(cnts) if c==0][:20])