"""Copy non-system dylib deps of the given Mach-O files into a Frameworks dir
and rewrite every reference to @executable_path/../Frameworks/<name>.
usage: bundle_libs.py <frameworks dir> <mach-o>..."""
import os, shutil, subprocess, sys
fw = sys.argv[1]; todo = list(sys.argv[2:]); done = set()
PFX = '@executable_path/../Frameworks/'
def deps(f):
    out = subprocess.run(['otool', '-L', f], capture_output=True, text=True).stdout.splitlines()[1:]
    return [l.strip().split(' (')[0] for l in out]
def rpaths(f):
    out = subprocess.run(['otool', '-l', f], capture_output=True, text=True).stdout.splitlines()
    r = []
    for i, l in enumerate(out):
        if 'LC_RPATH' in l:
            r.append(out[i + 2].strip().split()[1])
    return r
def resolve(d, f):
    if d.startswith('@rpath/'):
        for rp in rpaths(f) + ['/opt/homebrew/lib']:
            rp = rp.replace('@loader_path', os.path.dirname(os.path.realpath(f)))
            c = os.path.join(rp, d[7:])
            if os.path.exists(c): return c
        return None
    if d.startswith('@loader_path/'):
        return os.path.join(os.path.dirname(os.path.realpath(f)), d[13:])
    return d
while todo:
    f = todo.pop()
    if f in done: continue
    done.add(f)
    os.chmod(f, 0o755)
    for d in deps(f):
        if d.startswith(('/usr/lib/', '/System/')): continue
        if d.startswith(PFX):
            continue
        src = resolve(d, f)
        name = os.path.basename(d)
        if src is None or not os.path.exists(src):
            # self id or already-bundled name
            if os.path.exists(os.path.join(fw, name)):
                src = os.path.join(fw, name)
            else:
                sys.exit(f'unresolved {d} in {f}')
        dst = os.path.join(fw, name)
        if os.path.basename(f) == name and os.path.dirname(f) == fw:
            subprocess.run(['install_name_tool', '-id', PFX + name, f], check=True, capture_output=True)
            continue
        if not os.path.exists(dst):
            shutil.copy2(os.path.realpath(src), dst); os.chmod(dst, 0o755)
            subprocess.run(['install_name_tool', '-id', PFX + name, dst], check=True, capture_output=True)
            todo.append(dst)
        subprocess.run(['install_name_tool', '-change', d, PFX + name, f], check=True, capture_output=True)
    for rp in rpaths(f):
        if rp.startswith(('/opt/homebrew', '/usr/local')):
            subprocess.run(['install_name_tool', '-delete_rpath', rp, f], capture_output=True)
print(len(os.listdir(fw)), 'libs')
