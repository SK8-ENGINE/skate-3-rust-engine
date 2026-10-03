"""Census: which sample-group entries of the banks' player objects reference multichannel samples, and
their azimuth bytes (which descriptor word feeds which panner parameter for 2/4/6-channel voices).

usage: py -3.13 tools/audio-file-inspect/multichannel_census.py [banks dir]
  default .local/audio-file-inspect/banks (from bank_layout_check.py --extract)
"""
import struct
import sys
from collections import Counter
from pathlib import Path

if len(sys.argv) > 1 and sys.argv[1] in ('-h', '--help'):
    sys.exit(__doc__)
BANKS = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2] / '.local/audio-file-inspect/banks'


def u8(d, o): return d[o]
def u16(d, o): return struct.unpack_from('>H', d, o)[0]
def u32(d, o): return struct.unpack_from('>I', d, o)[0]


def main():
    channels = Counter()
    examples = {}
    for path in sorted(BANKS.glob('*.abk')):
        d = path.read_bytes()
        sfx = u32(d, 0x20)
        cap = u32(d, sfx + 8)
        slots = []
        for i in range(cap):
            off = u32(d, sfx + 12 + 4 * i)
            if off == 0xFFFFFFFF:
                break
            h = u32(d, sfx + off)
            slots.append(((h >> 18) & 0x3F) + 1)
        at = u32(d, 0x1C)
        for _ in range(u16(d, 0x0A)):
            nplayers, nctl = u8(d, at + 0x24), u8(d, at + 0x27)
            tmpl = u32(d, at + 0x2C)
            for p in range(nplayers):
                obj = tmpl + u32(d, at + 0x3C + 4 * p)
                group = u32(d, obj + 4)
                for k in range(u32(d, group)):
                    e = group + 4 + 12 * k
                    s = struct.unpack_from('>h', d, e)[0]
                    if s < 0 or s >= len(slots):
                        continue
                    ch = slots[s]
                    channels[ch] += 1
                    if ch > 1:
                        examples.setdefault((path.name, ch), []).append(list(d[e + 3:e + 9]))
            at += 0x3C + 4 * (nplayers + nctl)
    print('entries by channel count:', dict(channels))
    for (name, ch), azs in sorted(examples.items()):
        print(f'{name} ch={ch} entries={len(azs)} az bytes (first 3): {azs[:3]}')


if __name__ == '__main__':
    main()
