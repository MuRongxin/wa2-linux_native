import struct, sys
path = sys.argv[1] if len(sys.argv)>1 else "fon.pak"
f=open(path,'rb').read()
print('filesize', len(f))
assert f[:4]==b'KCAP', f[:4]
# 假设 header: magic(4) + 8字节 + file_count(4) @off12; entries @off16 每项36字节
fc = struct.unpack('<I', f[12:16])[0]
print('file_count(off12)=', fc)
off=16
ents=[]
ok=True
for i in range(fc):
    if off+36>len(f):
        print('ran off table at',i); ok=False; break
    typ=struct.unpack('<I', f[off:off+4])[0]
    raw=f[off+4:off+28]
    name=raw.split(b'\x00')[0].decode('shift_jis','replace')
    eoff=struct.unpack('<I', f[off+28:off+32])[0]
    esize=struct.unpack('<I', f[off+32:off+36])[0]
    ents.append((typ,name,eoff,esize))
    off+=36
print('parsed',len(ents),'entries; next off after table=',off)
for e in ents[:12]:
    print(e)
if ents:
    last=ents[-1]
    print('last offset+size=',last[2]+last[3],'filesize=',len(f),'match=',last[2]+last[3]==len(f))
    e=ents[0]
    print('entry0 type',e[0],'off',e[2],'size',e[3])
    if e[0]==1 and e[2]+8<=len(f):
        sc,so=struct.unpack('<II', f[e[2]:e[2]+8])
        print('  comp header: size_comp=',sc,'size_orig=',so,'comp_data_len=',sc-8)
