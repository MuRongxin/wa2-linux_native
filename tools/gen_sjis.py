out = '/home/lrxin/wa2-native/src/codec/sjis_table.rs'
fwd = {}
for cp in range(0x20, 0x10000):
    try:
        bs = chr(cp).encode('cp932')
    except Exception:
        continue
    if len(bs) == 1:
        fwd[(0 << 8) | bs[0]] = cp
    elif len(bs) == 2:
        fwd[(bs[0] << 8) | bs[1]] = cp
items = sorted(fwd.items())
lines = []
lines.append('//! Auto-generated CP932 (Shift-JIS) -> Unicode mapping.')
lines.append('//! 0x00xx = single byte, 0xLLTT = double byte.')
lines.append('pub const SJIS_TO_UNICODE: &[(u16, u32)] = &[')
for k, v in items:
    lines.append('    (0x%04Xu16, 0x%04Xu32),' % (k, v))
lines.append('];')
lines.append('')
open(out, 'w').write(chr(10).join(lines))
print('entries=%d' % len(items))