
import struct
data = open('/tmp/script_out/1001.bnr','rb').read()
print('len', len(data))
print('first 4:', data[:4])
print('TPC32 at:', data.find(b'TPC32'))
print('LSCR count:', data.count(b'LSCR'))
# Parse after LSCR
off=4
def u32(o): return struct.unpack('<I', data[o:o+4])[0]
def u16(o): return struct.unpack('<H', data[o:o+2])[0]
print('--- after LSCR ---')
for i in range(8):
    print(f'off {off}: u32={u32(off)} (0x{u32(off):08x})')
    off+=4
print('--- scan for TPC32 context ---')
idx = data.find(b'TPC32')
if idx>0:
    print('found TPC32 at', idx)
    o=idx
    for i in range(12):
        print(f'off {o}: u32={u32(o)}')
        o+=4
# Try to find opcode-like repeating patterns: small u16 values
print('--- sample of u16 stream from off 4 ---')
o=4
vals=[]
for i in range(40):
    vals.append(u16(o)); o+=2
print(vals)
