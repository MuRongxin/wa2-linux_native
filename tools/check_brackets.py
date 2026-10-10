code = open('/tmp/wa2_test.js').read()
depth = 0
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
    if c == '{': depth += 1
    elif c == '}': depth -= 1
    elif c == '(': depth += 0.001
    elif c == ')': depth -= 0.001
    if depth < 0:
        print(f'Unbalanced at line {line}, char {repr(c)}, depth={depth}')
        break
    i += 1
print(f'Final depth: {depth}')
