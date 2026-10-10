
import csv
data = open('/tmp/script_out/1001.txt','rb').read().decode('cp932')
rows = data.split(chr(10))
print('total rows', len(rows))
for ri, row in enumerate(rows[:14]):
    fields = row.split(',')
    print('--- row', ri, ': ', len(fields), 'fields ---')
    for fi, f in enumerate(fields):
        disp = f if len(f) < 40 else f[:40] + '...'
        print('  [' + str(fi) + '] ' + repr(disp))
