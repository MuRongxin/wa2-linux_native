txt = open('/home/lrxin/wa2tool/extract/script/1001.txt', 'rb').read()
for v in [137, 159, 170, 146, 23]:
    print(f'--- txt byte {v} ---')
    print(repr(txt[max(0,v-10):v+40].decode('shift-jis', 'replace')))