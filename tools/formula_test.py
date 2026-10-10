def sjis_code(ch):
    b = ch.encode('shift-jis')
    return (b[0] << 8) | b[1]

# measured: real_cell(Y) = my_cell(Y) + k, my_cell = 224 + idx_in_expansion
# let me instead compare: real_cell(Y) = cell_M(X_observed) where X maps onto Y
# data: (Y, my_cell(Y), real_cell(Y))
data = [('奇', 980), ('采', 1514), ('哨', 1844), ('碩', 2084), ('設', 2090), ('占', 2101)]
# my_cell(Y) = 224 + table_idx(Y); table idx from charset3
s3 = open('/tmp/wa2_font_charset3.txt', encoding='utf-8').read()
for y, expected_my in data:
    cell_my = s3.find(y)
    print(f'{y}: code={hex(sjis_code(y))}  my_cell(lookup)={cell_my}', end='  ')
    print()
print()
# Candidate formula 1: cell = 189*(L-0x88) + (T-0x40) + C  -> fit C
print('=== test formula cell = 189*(L-0x88)+(T-0x40)+C ===')
samples = [('奇', 1204), ('采', 1738), ('哨', 2068), ('碩', 2308), ('設', 2314), ('占', 2325)]
# (these are the cells where the atlas ACTUALLY shows these chars:
#  cell_M(X) values from the render: 希->1208(奇), 菜->1745(采), 小->2077(哨), 雪->2318(碩), 千->2324(設), 泉->2335(占))
real = {'奇':1208, '采':1745, '哨':2077, '碩':2318, '設':2324, '占':2335}
Cs = []
for y in ['奇','采','哨','碩','設','占']:
    c = sjis_code(y)
    L, T = c >> 8, c & 0xFF
    raw = 189*(L-0x88) + (T-0x40)
    C = real[y] - raw
    Cs.append(C)
    print(f'{y}: code={hex(c)} raw={raw} real={real[y]} C={C}')
if len(set(Cs)) == 1:
    print('>>> CONSTANT C =', Cs[0])
else:
    print('C values:', Cs)

# Candidate formula 2: compact with 189-block alignment: cell = base + (block)*189 + (T-T0)
print()
print('=== check: does real-cell spacing follow 189 blocks? ===')
for y in ['奇','采','哨','碩','設','占']:
    c = sjis_code(y)
    L, T = c >> 8, c & 0xFF
    print(f'{y}: L={hex(L)} T={hex(T)} real={real[y]}  raw189={189*(L-0x88)+(T-0x40)}')