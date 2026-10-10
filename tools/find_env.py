import os, glob

# find wine prefixes and launcher scripts
for pat in ['/home/lrxin/.wine*', '/home/lrxin/wa2*', '/home/lrxin/Downloads/WHITE ALBUM/*']:
    for p in glob.glob(pat):
        print('PATH:', p)

print()
# launcher scripts in the game dirs
for d in glob.glob('/home/lrxin/Downloads/WHITE ALBUM/*/'):
    for f in glob.glob(os.path.join(d, '*.sh')):
        print('SCRIPT:', f)
        try:
            print('   ', open(f).read()[:600].replace('\n', ' | '))
        except Exception as e:
            print('   read fail', e)

print()
# save dirs
for pat in ['/home/lrxin/.wine*/drive_c/**/save*', '/home/lrxin/Downloads/WHITE ALBUM/**/save*',
            '/home/lrxin/Downloads/WHITE ALBUM/**/*.sav', '/home/lrxin/.wine*/*savedata*']:
    hits = glob.glob(pat, recursive=True)[:10]
    if hits:
        print(f'{pat}:')
        for h in hits[:10]:
            print('   ', h)