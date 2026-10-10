def sjis_code(ch):
    b = ch.encode('shift-jis')
    return (b[0] << 8) | b[1] if len(b) == 2 else None

def raw(y):
    c = sjis_code(y)
    L, T = c >> 8, c & 0xFF
    return 189 * (L - 0x88) + (T - 0x40)

# New render (formula applied): intended -> shown
pairs = [('春','嫉'), ('希','忌'), ('小','嫉'), ('雪','舌'), ('菜','蔵'), ('和','亜'), ('泉','洗'), ('千','置'), ('晶','梢')]
print('=== formula_cell(X) = 655 + raw(X) should equal real_cell(Y) ===')
print('If atlas[655+raw(X)] == Y, then 655+raw(X) = C_Y + raw(Y) for the SAME C_Y')
for x, y in pairs:
    cx = sjis_code(x); cy = sjis_code(y)
    rx = 189*(cx>>8-0x88) + ((cx & 0xFF)-0x40)
    ry = 189*(cy>>8-0x88) + ((cy & 0xFF)-0x40)
    # atlas cell shown = 655 + rx  =>  C_Y = (655 + rx) - ry
    print(f'{x}->{y}: rawX={rx} rawY={ry}  implied C_Y = {655 + rx - ry}')

# Also: previous render (charset3 map): intended -> shown, with my_cell = 224+tableidx
s3 = open('/tmp/wa2_font_charset3.txt', encoding='utf-8').read()
pairs2 = [('春','尞'), ('希','奇'), ('小','哨'), ('雪','碩'), ('菜','采'), ('和','楞'), ('泉','占'), ('千','設'), ('晶','彫')]
print()
print('=== charset3 render: my_cell(X)=find(X); implied C_Y = my_cell(X) - raw(Y) ===')
for x, y in pairs2:
    my = s3.find(x)
    cy = sjis_code(y)
    if cy is None:
        print(f'{x}->{y}: Y not 2-byte')
        continue
    ry = 189*(cy>>8-0x88) + ((cy & 0xFF)-0x40)
    print(f'{x}->{y}: my_cell(X)={my} rawY={ry}  implied C_Y = {my - ry}')