s3 = open('/tmp/wa2_font_charset3.txt', encoding='utf-8').read()
s2 = open('/tmp/wa2_font_charset2.txt', encoding='utf-8').read()
pairs = [('春','尞'), ('希','奇'), ('と','セ'), ('小','哨'), ('春','肅'), ('。','＆'),
         ('雪','碩'), ('菜','采'), ('、','＃'), ('和','楞'), ('泉','占'), ('千','設'), ('晶','彫')]
print('=== charset3 deltas (rendered_cell - intended_cell) ===')
for a, b in pairs:
    ia = s3.find(a); ib = s3.find(b)
    print(f'{a}({ia}) -> {b}({ib})   delta={ib-ia if ib>=0 else "?"}')

# Also: where are these rendered chars in the ORIGINAL table order (core)?
core = s2[95:]
print()
print('=== in-table index deltas (core idx) ===')
for a, b in pairs:
    ia = core.find(a); ib = core.find(b)
    print(f'{a}(core {ia}) -> {b}(core {ib})   delta={ib-ia if ib>=0 else "?"}')