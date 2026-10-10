import glob
chars=set()
for fn in glob.glob('/tmp/script_out/*.txt'):
    try: chars.update(open(fn,'rb').read().decode('cp932'))
    except: pass
chars={c for c in chars if ord(c)>0x7f}
cht=open('/tmp/wa2.cht.txt','r',encoding='utf-8').read()
cht_set={c for c in cht if not c.isspace()}
print('script unique non-ascii:', len(chars))
print('wa2.cht.txt non-space chars:', len(cht_set))
overlap=chars & cht_set
print('script chars present in wa2.cht.txt:', len(overlap), '(%.1f%%)'%(100*len(overlap)/len(chars)))
kana={c for c in chars if len(c.encode('cp932','ignore'))==2 and c.encode('cp932','ignore')[0]<0x88}
print('  of which kana(lead<0x88):', len(kana & cht_set), 'in cht /', len(kana), 'total kana')
kanji=chars-kana
print('  kanji:', len(kanji), '-> in cht:', len(kanji & cht_set), '(%.1f%%)'%(100*len(kanji & cht_set)/max(1,len(kanji))))