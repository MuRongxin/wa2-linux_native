import os, struct, re
root = '/home/lrxin/Downloads/WHITE ALBUM/白色相簿2 整合版'
print('=== non-pak files ===')
for f in sorted(os.listdir(root)):
    p = os.path.join(root, f)
    if os.path.isfile(p) and not f.lower().endswith('.pak'):
        print(f'  {f}  {os.path.getsize(p)}')
for sub in ('grp', 'IC'):
    d = os.path.join(root, sub)
    if os.path.isdir(d):
        fs = sorted(os.listdir(d))
        print(f'=== {sub}/ ({len(fs)} files) ===')
        print('  ', fs[:25])