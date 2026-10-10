import ctypes, os
os.environ['SDL_AUDIODRIVER']='pulse'
l=ctypes.CDLL('libSDL2-2.0.so.0')
l.SDL_Init.argtypes=[ctypes.c_uint32]; l.SDL_Init.restype=ctypes.c_int
l.SDL_InitSubSystem.argtypes=[ctypes.c_uint32]; l.SDL_InitSubSystem.restype=ctypes.c_int
l.SDL_GetError.restype=ctypes.c_char_p
l.SDL_OpenAudioDevice.argtypes=[ctypes.c_char_p,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_int]; l.SDL_OpenAudioDevice.restype=ctypes.c_uint32
l.SDL_GetCurrentAudioDriver.restype=ctypes.c_char_p
print('SDL_Init(VIDEO|AUDIO=0x30):', l.SDL_Init(0x20|0x10), 'err=', l.SDL_GetError())
print('GetCurrentAudioDriver:', l.SDL_GetCurrentAudioDriver())
dev=l.SDL_OpenAudioDevice(None, 0x8010, (48000<<15)|2, 0, 0)
print('OpenAudioDevice ->', dev, 'err=', l.SDL_GetError())
print('GetCurrentAudioDriver after open:', l.SDL_GetCurrentAudioDriver())