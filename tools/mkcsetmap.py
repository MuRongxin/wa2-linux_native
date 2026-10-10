s=open('/tmp/wa2_cset.txt',encoding='utf-8',errors='ignore').read()
nz=[c for c in s if not c.isspace()]
print('wa2_cset nonspace:', len(nz))
with open('/tmp/cset_map.txt','w') as f:
    for i,ch in enumerate(nz):
        f.write(ch+'\t'+str(748+i)+'\n')
print('wrote cset_map (748+i)')