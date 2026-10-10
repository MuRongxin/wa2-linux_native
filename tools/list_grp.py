
import struct
def list_kcap(path, ext_filter=None, limit=40):
    with open(path,'rb') as f:
        sig = f.read(4)
        assert sig==b'KCAP', sig
        f.read(12)  # unknown1, unknown2, entry_count
        entry_count = struct.unpack('<I', sig[:0]+b'' )[0] if False else None
        # re-read properly
    with open(path,'rb') as f:
        data=f.read()
    entry_count = struct.unpack('<I', data[12:16])[0]
    off=16
    out=[]
    for i in range(entry_count):
        e=data[off:off+44]
        flag=struct.unpack('<I',e[0:4])[0]
        name=e[4:28].split(b'\x00')[0]
        length=struct.unpack('<I',e[40:44])[0]
        out.append((i, name.decode('shift_jis','replace'), length))
        off+=44
    return out

import sys
for pak in ['/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/grp.pak']:
    print('=== ',pak)
    entries=list_kcap(pak)
    amps=[e for e in entries if e[1].lower().endswith('.amp') or e[1].lower().endswith('.bbm')]
    tgas=[e for e in entries if e[1].lower().endswith('.tga')]
    print('total',len(entries),'amp/bbm',len(amps),'tga',len(tgas))
    for e in amps[:30]:
        print(' AMP', e)
    print('--- sample tgas ---')
    for e in tgas[:10]:
        print(' TGA', e)
