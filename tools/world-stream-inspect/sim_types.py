"""Survey RW4 arena dictionary types in a district's simulation streams (cSim_*.xsf).

usage: py -3.13 tools/world-stream-inspect/sim_types.py DIST_SkateSchool [--find HEX64 ...] [--dump TYPEHEX] [--disc DIR]
Reads straight from <disc>/data/content/world<District>.big (nothing extracted to disk).
Dictionary entry = 6 BE u32: (offset, ?, size, align?, ?, type). Prints type counts per stream file;
--find searches every arena for a 64-bit value (both word orders) and names the entry holding it;
--dump hex-dumps the first 256 bytes of every entry of one type.
"""
import argparse
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import arenas  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description='Survey RW4 arena dictionary types in a district.')
    parser.add_argument('district', help='e.g. DIST_SkateSchool')
    parser.add_argument('--find', action='append', default=[], metavar='HEX64', help='64-bit value to search for')
    parser.add_argument('--dump', metavar='TYPEHEX', help='hex-dump the entries of this type id')
    parser.add_argument('--disc', type=Path, default=arenas.default_disc(), help='extracted disc root (default: %(default)s)')
    args = parser.parse_args()
    finds = [int(v, 16) for v in args.find]
    dump = int(args.dump, 16) if args.dump else None
    total = Counter()
    for fname, aid, d in arenas.district_arenas(args.disc, args.district):
        ents = arenas.entries(d)
        c = Counter(e[5] for e in ents)
        total.update(c)
        if not finds and dump is None:
            print(fname, '%016x' % aid, ' '.join('%08x:%d' % kv for kv in sorted(c.items())))
        for v in finds:
            for pat, tag in ((v.to_bytes(8, 'big'), 'BE'), (((v & 0xFFFFFFFF) << 32 | v >> 32).to_bytes(8, 'big'), 'LOWFIRST')):
                i = d.find(pat)
                while i >= 0:
                    owner = [k for k, e in enumerate(ents) if e[0] <= i < e[0] + e[2]]
                    print('FIND %016x %s %s @%x in entry %s type %s' % (v, tag, fname, i, owner, [hex(ents[k][5]) for k in owner]))
                    i = d.find(pat, i + 1)
        if dump is not None:
            for k, e in enumerate(ents):
                if e[5] == dump:
                    print(fname, k, 'off %x size %x' % (e[0], e[2]))
                    blob = d[e[0]:e[0] + min(e[2], 256)]
                    for j in range(0, len(blob), 32):
                        print('   ', blob[j:j + 32].hex(' ', 4))
    print('TOTAL', ' '.join('%08x:%d' % kv for kv in sorted(total.items())))


if __name__ == '__main__':
    main()
