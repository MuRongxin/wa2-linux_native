import struct

def list_kcap(path, limit=40):
    f = open(path, 'rb')
    hdr = f.read(16)
    assert hdr[:4] == b'KCAP', path
    n = struct.unpack_from('<I', hdr, 12)[0]
    print(f'{path}: {n} entries')
    names = []
    for i in range(n):
        f.seek(16 + i*44)
        e = f.read(44)
        comp = struct.unpack_from('<I', e, 0)[0]
        name = e[4:28].split(b'\x00')[0]
        off = struct.unpack_from('<I', e, 36)[0]
        size = struct.unpack_from('<I', e, 40)[0]
        try:
            nm = name.decode('shift-jis')
        except Exception:
            nm = repr(name)
        names.append((nm, comp, off, size))
    for nm, comp, off, size in names[:limit]:
        print(f'  c={comp} {nm}  size={size}')
    return names, f

names, f = list_kcap('/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版/char.pak', 30)
# stats on extensions
from collections import Counter
exts = Counter(nm.rsplit('.',1)[-1].lower() if '.' in nm else '?' for nm,_,_,_ in names)
print('extensions:', exts)
# any names containing chr/tachie
sus = [nm for nm,_,_,_ in names if 'chr' in nm.lower() or 'tachi' in nm.lower()]
print('chr-ish:', sus[:10], 'count', len(sus))