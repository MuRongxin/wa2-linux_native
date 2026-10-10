
import shutil, os
# Original Japanese atlas (fnt.pak) - complete Japanese glyphs
src_jp = None
for root,_,files in os.walk('/tmp/fnt_out'):
    for f in files:
        if '14pt' in f and f.lower().endswith('.tga'):
            src_jp = os.path.join(root,f); break
    if src_jp: break
# Patched atlas (fon.pak) - Chinese at 748+
src_cn = None
for root,_,files in os.walk('/tmp/fon_out'):
    for f in files:
        if '14pt' in f and f.lower().endswith('.tga'):
            src_cn = os.path.join(root,f); break
    if src_cn: break
print('jp src', src_jp, 'cn src', src_cn)
if src_jp: shutil.copy(src_jp, '/tmp/font_jp.tga')
if src_cn: shutil.copy(src_cn, '/tmp/font_cn.tga')
print('copied')
