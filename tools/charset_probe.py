
import unicodedata
data = open('/tmp/wa2.cht.txt','rb').read()
# It may be utf-8 or utf-16 or shift-jis
for enc in ['utf-8','utf-16','shift_jis']:
    try:
        s = data.decode(enc)
        print('ENC', enc, 'len', len(s))
        chars = [c for c in s if not c.isspace()]
        print('non-space chars:', len(chars))
        # check sorted by codepoint
        cps = [ord(c) for c in chars]
        is_sorted = all(cps[i] <= cps[i+1] for i in range(len(cps)-1))
        print('sorted_by_codepoint:', is_sorted)
        print('first 20 chars:', repr(''.join(chars[:20])))
        print('first 20 cps:', cps[:20])
        break
    except Exception as e:
        print('enc', enc, 'fail', e)
