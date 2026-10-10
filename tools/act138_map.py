import struct, glob, os
from collections import Counter, defaultdict

per_speaker = defaultdict(Counter)
per_pair = defaultdict(Counter)

for path in sorted(glob.glob('/home/lrxin/wa2tool/extract/script/*.bnr')):
    bnr = open(path, 'rb').read()
    txt = open(path[:-4] + '.txt', 'rb').read()
    tokens = txt.split(b',')
    n = len(bnr)
    def w(off): return struct.unpack_from('<I', bnr, off)[0]
    def tok(nn):
        return tokens[nn].decode('shift-jis', 'replace') if nn < len(tokens) else '?'

    i = 28
    cur_tok = None
    cur_vals = []
    last_speaker = None
    while i + 12 <= n:
        op = w(i)
        if op == 6:
            cur_tok = None
            cur_vals = []
            i += 8
        elif op == 5:
            cur_vals.append(w(i + 8))
            i += 12
        elif op == 3:
            cur_tok = w(i + 4)
            i += 8
        elif op == 4:
            act = w(i + 4)
            if act == 144 and cur_tok is not None:
                last_speaker = tok(cur_tok)
            elif act == 138 and len(cur_vals) >= 5 and last_speaker:
                p0, p1, p2, p3, p4 = cur_vals[-5:]
                per_speaker[last_speaker][(p0, p4)] += 1
                per_pair[(p0, p4)][last_speaker] += 1
            i += 8
        elif op == 0:
            sub = w(i + 4)
            i += 12 if sub == 12 else (16 if sub in (1, 2) else 8)
        elif op in (1, 2):
            i += 8
        else:
            i += 4

print('=== per speaker: top (p0, p4) ===')
for sp, c in sorted(per_speaker.items(), key=lambda kv: -sum(kv[1].values()))[:24]:
    top = ', '.join(f'{k}:{v}' for k, v in c.most_common(4))
    print(f'  {sp!r}: n={sum(c.values())}  {top}')

print()
print('=== per (p0,p4): top speakers ===')
for k, c in sorted(per_pair.items(), key=lambda kv: -sum(kv[1].values()))[:28]:
    top = ', '.join(f'{s}:{v}' for s, v in c.most_common(3))
    print(f'  {k}: {top}')