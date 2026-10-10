import os, shutil, glob
d='/tmp/wa2_fnt'
res=[]
for e in os.scandir(d):
    if e.is_file() and e.name.lower().endswith('.tga'):
        tgt='/tmp/wa2fnt_'+str(len(res))+'.tga'
        shutil.copy(e.path, tgt)
        res.append((os.path.getsize(tgt), tgt))
for s,f in sorted(res):
    print(s, f)