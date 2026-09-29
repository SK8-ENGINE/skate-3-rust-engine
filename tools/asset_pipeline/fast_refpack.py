"""Optional bundled native RefPack backend; source-only tools retain Python."""
import ctypes, sys
from pathlib import Path

_native_names = ('refpack.dll', 'librefpack.dylib', 'refpack.dylib', 'librefpack.so', 'refpack.so')
if sys.platform == 'darwin':
    _native_names = ('librefpack.dylib', 'refpack.dylib', 'refpack.dll', 'librefpack.so', 'refpack.so')
elif sys.platform.startswith('linux'):
    _native_names = ('librefpack.so', 'refpack.so', 'refpack.dll', 'librefpack.dylib')

_library = None
_candidates = [Path(__file__).with_name(n) for n in _native_names]
_candidates += [Path(__file__).resolve().parents[2] / 'target/native' / n for n in _native_names]
for path in _candidates:
    if path.is_file():
        try:
            _library = ctypes.CDLL(str(path))
        except OSError:
            continue
        _library.skate_refpack.argtypes=[ctypes.c_char_p,ctypes.c_size_t,ctypes.c_void_p,ctypes.c_size_t,ctypes.c_size_t,ctypes.c_bool]
        _library.skate_refpack.restype=ctypes.c_int
        break

def decode(data,size,start,early=False):
    if _library is None:return None
    output=ctypes.create_string_buffer(size)
    if _library.skate_refpack(data,len(data),output,size,start,early):
        raise ValueError('Malformed RefPack command or declared output size')
    return output.raw
