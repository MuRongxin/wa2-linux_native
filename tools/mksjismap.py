import sys
s=open('/tmp/wa2_sjis_charset.txt',encoding='utf-8',errors='ignore').read()
nz=[c for c in s if not c.isspace()]
print('total sjis chars:', len(nz))
# hypothesis: atlas cells 0..N correspond to sjis chars in code order
N=7680
with open('/tmp/sjis_map.txt','w') as f:
    for i,ch in enumerate(nz[:N]):
        f.write(ch+'\t'+str(i)+'\n')
print('wrote /tmp/sjis_map.txt with', min(len(nz),N), 'entries')
# sample: where do common chars land?
for ch in ['あ','い','ア','一','人','日','。','、']:
    if ch in nz: print('  ', ch, '-> cell', nz.index(ch))