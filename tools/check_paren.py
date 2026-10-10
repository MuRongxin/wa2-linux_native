code = open('/tmp/wa2_test.js').read()
paren = 0
in_str = None
in_comment = False
line = 1
i = 0
last_paren_line = 0
while i < len(code):
    c = code[i]
    if c == '\n': line += 1
    if in_comment:
        if c == '\n': in_comment = False
        i += 1; continue
    if in_str:
        if c == '\\': i += 2; continue
        if c == in_str: in_str = None
        i += 1; continue
    if c == '/' and i+1 < len(code) and code[i+1] == '/':
        in_comment = True; i += 2; continue
    if c in '"\'`':
        in_str = c; i += 1; continue
    if c == '(':
        paren += 1
        last_paren_line = line
    elif c == ')':
        paren -= 1
        if paren < 0:
            lines = code.split('\n')
            print(f'Extra ) at line {line}')
            for j in range(max(0,line-3), min(len(lines), line+1)):
                print(f'  {j+1}: {lines[j][:130]}')
            print(f'  Last ( was at line {last_paren_line}')
            break
    i += 1
print(f'Final paren: {paren}')
