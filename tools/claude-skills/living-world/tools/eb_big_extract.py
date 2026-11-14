"""Extract files from a Skate 3 'EB' v3 .big archive (e.g. data/big/shaders_final.big).

Layout (worked out 2026-10-07 on shaders_final.big, big endian):
  0x00 'EB' u16 version 3, u32 file count, ... u32 name table offset (0x1a40 there)
  0x30 TOC: count x 16 bytes (u32 offset in 64-byte units, u32 0, u32 size, u32 hash)
  name table: count x 55 bytes (u16 prefix, then the NUL-padded name), same order as the TOC

Usage: eb_big_extract.py <archive.big> <out_dir> [name_prefix ...]   (no prefix = list names)
Then read shader microcode with xenos_disasm.py next to this file.
"""
import os
import struct
import sys


def entries(b):
    count = struct.unpack_from('>I', b, 4)[0]
    names = struct.unpack_from('>I', b, 12)[0]
    for i in range(count):
        off, _, size, _ = struct.unpack_from('>IIII', b, 0x30 + 16 * i)
        raw = b[names + i * 55 + 2:names + (i + 1) * 55]
        yield raw.split(b'\0')[0].decode(), off * 64, size


def main(path, out, prefixes):
    b = open(path, 'rb').read()
    for name, off, size in entries(b):
        if not prefixes:
            print(name, size)
        elif any(name.startswith(p) for p in prefixes):
            os.makedirs(out, exist_ok=True)
            open(os.path.join(out, name), 'wb').write(b[off:off + size])
            print('wrote', name, size)


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2], sys.argv[3:])
