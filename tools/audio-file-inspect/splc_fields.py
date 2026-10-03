r"""Census of SPLC (.bnk) record/group/member fields on the disc banks (our own reading tool).
usage: py -3.13 tools/audio-file-inspect/splc_fields.py <dir with .bnk files> [bank stem] [record id]
(extract the .bnk files with tools/world-stream-inspect/big_list.py audiofiles.big "\.bnk$" --out DIR)
Prints value ranges for every 4-byte member offset (as f32 and u32), group header words, record
header words; with a record id, dumps that record's groups and members raw."""
import struct, sys
from collections import Counter, defaultdict
from pathlib import Path


def u32(d, o):
    return struct.unpack_from('>I', d, o)[0]


def f32(d, o):
    return struct.unpack_from('>f', d, o)[0]


def walk(d):
    records, containers = u32(d, 12), u32(d, 16)
    cbase = 60 + 36 * records
    cursor = cbase + 72 * containers
    out = []
    for r in range(records):
        at = 60 + 36 * r
        groups = []
        for _ in range(d[at + 7]):
            g = cursor
            count = d[g + 8]
            cursor += 12
            members = []
            for _ in range(count):
                members.append(cursor)
                cursor += 72
            groups.append((g, members))
        out.append((at, groups))
    return out, cbase, containers


def main():
    root = Path(sys.argv[1])
    only = sys.argv[2] if len(sys.argv) > 2 else None
    rec = int(sys.argv[3]) if len(sys.argv) > 3 else None
    stats = defaultdict(Counter)
    for p in sorted(root.glob('*.bnk')):
        d = p.read_bytes()
        if d[:4] != b'SPLC' or (only and p.stem != only):
            continue
        recs, cbase, nc = walk(d)
        if rec is not None:
            at, groups = recs[rec] if rec < len(recs) else (None, None)
            if at is None:
                c = cbase + 72 * (rec - len(recs))
                print('container', d[c:c + 72].hex(' '))
                return
            print('record', d[at:at + 36].hex(' '))
            for g, ms in groups:
                print(' group', d[g:g + 12].hex(' '))
                for m in ms:
                    print('  member', ' '.join(f'{o}:{f32(d, m + o):.4g}/{u32(d, m + o):#x}' for o in range(0, 72, 4)))
            return
        for at, groups in recs:
            for o in range(0, 36, 4):
                stats[f'rec+{o}'][u32(d, at + o)] += 1
            for g, ms in groups:
                for o in (4, 8):
                    stats[f'grp+{o}'][u32(d, g + o)] += 1
                for m in ms:
                    for o in range(0, 72, 4):
                        stats[f'mem+{o}'][round(f32(d, m + o), 4) if o not in (0, 56, 60, 68) else hex(u32(d, m + o))] += 1
        for c in range(nc):
            stats['cont+0'][u32(d, cbase + 72 * c)] += 1
            stats['cont+68'][hex(u32(d, cbase + 72 * c + 68))] += 1
    for k, c in stats.items():
        print(k, len(c), c.most_common(12))


if __name__ == '__main__':
    if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    main()
