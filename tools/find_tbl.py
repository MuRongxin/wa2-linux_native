import glob, os
for f in sorted(glob.glob('/tmp/*.txt')+glob.glob('/tmp/*.cs')+glob.glob('/tmp/*.tbl')+glob.glob('/tmp/*.json')):
    try:
        s=open(f,encoding='utf-8',errors='ignore').read()
        n=len([c for c in s if not c.isspace()])
        if n>1500: print(n, os.path.getsize(f), f)
    except Exception as e: pass
print('--- .py scripts I/user made ---')
for f in sorted(glob.glob('/tmp/*.py')):
    print(os.path.getsize(f), f)