import struct
f='/tmp/grp_out/v208100.tga'
d=open(f,'rb').read()
print('filesize', len(d))
idlen,cmtype,imgtype = d[0],d[1],d[2]
cm_first,cm_len,cm_depth = struct.unpack('<HHB', d[3:8])
x0,y0,w,h = struct.unpack('<HHHH', d[8:16])
depth,desc = d[16],d[17]
print(f'idlen={idlen} cmtype={cmtype} imgtype={imgtype} cm(first={cm_first},len={cm_len},depth={cm_depth})')
print(f'{w}x{h} depth={depth} desc=0x{desc:02x}')
exp = 18+idlen + (cm_len*(cm_depth//8) if cm_len else 0) + w*h*((depth+7)//8)
print('expected size for 32bpp:', exp, ' -> matches' if exp==len(d) else ' MISMATCH')
exp24 = 18+idlen + (cm_len*(cm_depth//8) if cm_len else 0) + w*h*3
print('expected size for 24bpp:', exp24, ' -> matches' if exp24==len(d) else '')
print('expected size for 16bpp:', 18+idlen+w*h*2, ' -> matches' if 18+idlen+w*h*2==len(d) else '')