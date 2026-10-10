import json
m=json.load(open('/tmp/jp_map.json'))
with open('/tmp/jp_map.txt','w') as f:
    for ch,cell in m.items():
        f.write(ch+'\t'+str(cell)+'\n')
print('wrote', len(m), 'entries')