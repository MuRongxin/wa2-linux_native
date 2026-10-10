import sys
data = open('/tmp/script_out/1001.txt','rb').read()
print('total bytes', len(data))
print('first 60 bytes hex:', data[:60].hex())
for enc in ['shift_jis','cp932','gbk','gb18030','big5','euc_jp','utf-8']:
    try:
        s = data.decode(enc)
        print('---', enc, 'OK ---')
        print(repr(s[:100]))
    except Exception as e:
        print('---', enc, 'FAIL', e, '---')
