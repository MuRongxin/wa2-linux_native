s = open('/tmp/wa2_font_charset2.txt', encoding='utf-8').read()
pairs = [('春','耳'), ('希','積'), ('小','釈'), ('雪','新'), ('菜','恒'), ('和','浴'), ('泉','身'), ('千','申'), ('晶','置'), ('と','h'), ('。','イ')]
for intended, rendered in pairs:
    i_int = s.find(intended)
    i_ren = s.find(rendered)
    print(f'{intended}({i_int}) -> shown {rendered}({i_ren})   delta(rendered-intended) = {i_ren - i_int if i_ren>=0 else "?"}')