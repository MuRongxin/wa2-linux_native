import struct

def dump_head(name, upto_off):
    bnr = open(f'/home/lrxin/wa2tool/extract/script/{name}.bnr', 'rb').read()
    txt = open(f'/home/lrxin/wa2tool/extract/script/{name}.txt', 'rb').read()
    tokens = txt.split(b',')
    n = len(bnr)
    def w(off): return struct.unpack_from('<I', bnr, off)[0]
    def tok(nn): return tokens[nn].decode('shift-jis','replace')[:24] if nn < len(tokens) else 'oob'
    i = 28
    rec = None
    print(f'===== {name} records until {upto_off} =====')
    while i + 12 <= n and i < upto_off:
        op = w(i)
        if op == 6:
            rec = (i, w(i+4), []); i += 8
            print(f'  {rec[0]:6d} K{rec[1]:3d}:', end='')
        elif op == 5:
            t = w(i+4); v = w(i+8)
            if rec:
                if t == 4:
                    vv = struct.unpack('<f', struct.pack('<I', v))[0]
                    rec[2].append(f'f={vv:.1f}')
                else:
                    rec[2].append(f'i={v}')
            i += 12
        elif op == 3:
            if rec: rec[2].append(f"tok#{w(i+4)}({tok(w(i+4))!r})")
            i += 8
        elif op == 4:
            if rec: rec[2].append(f'ACT{w(i+4)}')
            i += 8
        elif op == 0:
            sub = w(i+4)
            if rec: rec[2].append(f'(0,{sub})')
            i += 12 if sub == 12 else (16 if sub in (1,2) else 8)
        elif op in (1, 2):
            if rec: rec[2].append(f'({op},{w(i+4)})')
            i += 8
        else:
            if rec: rec[2].append(f'RAW{w(i)}')
            i += 4
        if rec and rec[2] and i >= upto_off - 64:
            pass
    # print in order with grouping: re-walk simpler: print ops linearly
    i = 28
    print('--- linear op dump ---')
    cnt = 0
    while i + 12 <= n and i < upto_off:
        op = w(i)
        if op == 6:
            print(f'\nK{i}: kind={w(i+4)}')
            i += 8
        elif op == 5:
            t = w(i+4); v = w(i+8)
            if t == 4:
                vv = struct.unpack('<f', struct.pack('<I', v))[0]
                print(f'  f={vv:.1f}', end='')
            else:
                print(f'  i={v}', end='')
            i += 12
        elif op == 3:
            print(f'  tok#{w(i+4)}({tok(w(i+4))!r})', end='')
            i += 8
        elif op == 4:
            print(f'  ACT{w(i+4)}', end='')
            i += 8
        elif op == 0:
            sub = w(i+4)
            if sub == 12:
                print(f'  GOTO {w(i+8)}', end=''); i += 12
            elif sub in (1,2):
                print(f'  (0,{sub},{w(i+8)},{w(i+12)})', end=''); i += 16
            else:
                print(f'  (0,{sub})', end=''); i += 8
        elif op in (1,2):
            print(f'  ({op},{w(i+4)})', end=''); i += 8
        else:
            print(f'  RAW{w(i)}', end=''); i += 4
        cnt += 1
        if cnt > 120000: break
    print()

dump_head('1008', 4200)