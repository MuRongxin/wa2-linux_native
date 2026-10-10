code = open('/tmp/wa2_test.js').read()
brace = 0  # {}
paren = 0  # ()
bracket = 0  # []
in_str = None
in_comment = False
line = 1
i = 0
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
    if c == '{': brace += 1
    elif c == '}': brace -= 1
    elif c == '(': paren += 1
    elif c == ')': paren -= 1
    elif c == '[': bracket += 1
    elif c == ']': bracket -= 1
    if brace < 0:
        print(f'Brace unbalanced at line {line}, char {repr(c)}, brace={brace}')
        break
    if paren < 0:
        print(f'Paren unbalanced at line {line}, char {repr(c)}, paren={paren}')
        # print surrounding lines
        lines = code.split('\n')
        for j in range(max(0,line-3), min(len(lines), line+2)):
            print(f'  {j+1}: {lines[j][:120]}')
        break
    i += 1
print(f'Final: brace={brace}, paren={paren}, bracket={bracket}')
