
import os, shutil
def find_tga(d):
    for entry in os.scandir(d):
        if entry.is_file() and entry.name.lower().endswith('.tga'):
            if '14pt' in entry.name or '14PT' in entry.name.upper():
                return entry.path
    return None
jp = find_tga('/tmp/fnt_out')
cn = find_tga('/tmp/fon_out')
print('jp=',jp)
print('cn=',cn)
if jp: shutil.copy(jp, '/tmp/font_jp.tga')
if cn: shutil.copy(cn, '/tmp/font_cn.tga')
print('done')
