s = open('/tmp/wa2_font_charset.txt', encoding='utf-8').read()
print(f'len={len(s)}')
# find the ASCII block inside the table
i = s.find('A')
print('A at', i)
print('context around A:', repr(s[i-40:i+60]))
# look for the exact contiguous ASCII run 0x20..0x7E
target = ''.join(chr(c) for c in range(0x20, 0x7F))
pos = s.find(target)
print('full ASCII run 0x20-0x7E found at:', pos)
if pos < 0:
    # try to find a partial run
    for start in range(max(0, i-40), min(len(s), i+40)):
        if s[start:start+10] == target[:10]:
            print('ASCII-ish run starts at', start, repr(s[start:start+30]))
            break
# what's right after 0x7E (if found)?
if pos >= 0:
    print('after 0x7E:', repr(s[pos+95:pos+120]))
    print('before:', repr(s[max(0,pos-30):pos]))
# check for kana run around 210
print('around 210:', repr(s[200:240]))
print('around 524:', repr(s[515:540]))