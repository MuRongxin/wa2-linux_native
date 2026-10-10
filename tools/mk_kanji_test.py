test='\u4e00\u4e8c\u4e09\u5341\u4eba\u5927\u65e5\u6708\u5c71\u5ddd\u7530\u53e3\u76ee'
line = '<R>' + test
content = ',mv01,,,,' + line + ',CATCH,1002'
open('/tmp/kanji_test.txt','wb').write(content.encode('cp932'))
print('wrote', len(content), 'bytes')