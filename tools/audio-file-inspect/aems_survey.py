"""Independent survey of every ABKC bank (.abk) and MOIR project (.csi) in audiofiles.big: checks the
bank layout the native AEMS runtime (crates/skate-audio) relies on and prints histograms (records,
capacities, program opcodes, export kinds, voice objects, curve types). Our own reader.

usage: py -3.13 tools/audio-file-inspect/aems_survey.py [--dump BANK.abk] [--disc DIR]
Prints invariant checks and histograms; with --dump, a readable dump of one bank's records and programs.
"""
import argparse
import os
import struct
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.owned_game.big import BigArchive  # noqa: E402



def u8(d, o): return d[o]
def u16(d, o): return struct.unpack_from('>H', d, o)[0]
def s16(d, o): return struct.unpack_from('>h', d, o)[0]
def u32(d, o): return struct.unpack_from('>I', d, o)[0]
def s32(d, o): return struct.unpack_from('>i', d, o)[0]
def f32(d, o): return struct.unpack_from('>f', d, o)[0]


def cstr(d, o):
    e = d.index(b'\0', o)
    return d[o:e].decode('latin1')


class Csi:
    def __init__(self, name, d):
        self.name, self.d = name, d
        assert d[:4] == b'MOIR'
        self.counts = (u16(d, 0x0A), u16(d, 0x0C), u16(d, 0x0E))
        self.project = u16(d, 0x10)
        self.tables = []
        at = 0x28
        for t, n in enumerate(self.counts):
            stride = 16 if t == 2 else 12
            rows = []
            for i in range(n):
                r = at + stride * i
                if t == 2:
                    rows.append(dict(off=r, head=u32(d, r), value=s32(d, r + 4), name=cstr(d, u32(d, r + 8)),
                                     id=u16(d, r + 12), gen=u16(d, r + 14)))
                else:
                    rows.append(dict(off=r, head=u32(d, r), name=cstr(d, u32(d, r + 4)), id=u16(d, r + 8),
                                     gen=u16(d, r + 10)))
            self.tables.append(rows)
            at += stride * n
        self.pool = at


def rebased(d):
    """Apply the bank's rebase list with base 0, so rebased words read as bank offsets."""
    b = bytearray(d)
    rl = u32(d, 0x34)
    n = u32(d, rl)
    sites = set()
    for i in range(n):
        site = u32(d, rl + 4 + 4 * i)
        sites.add(site)
    return bytes(b), sites


def records(d):
    n = u16(d, 0x0A)
    at = u32(d, 0x1C)
    out = []
    for _ in range(n):
        r = dict(at=at, sym=u32(d, at + 4), symid=u32(d, at + 8), live=u16(d, at + 28), cap=u16(d, at + 30),
                 nsubs=u16(d, at + 32), nbc=u16(d, at + 34), nvoice=u8(d, at + 36), release=u8(d, at + 37),
                 payload=u8(d, at + 38), nheld=u8(d, at + 39), prog=u32(d, at + 40), tmpl=u32(d, at + 44),
                 tsize=u32(d, at + 48), triple=u32(d, at + 52), livehead=u32(d, at + 56))
        k = r['nvoice'] + r['nheld']
        r['objs'] = [u32(d, at + 60 + 4 * i) for i in range(k)]
        out.append(r)
        at += 60 + 4 * k
    return out


def program(d, r):
    pc, blk, ops = r['prog'], 24, []
    while d[pc] != 255:
        op, np_ = d[pc], d[pc + 1]
        pairs = [(s32(d, pc + 4 + 8 * p), s32(d, pc + 8 + 8 * p)) for p in range(np_)]
        adv = s32(d, pc + 4 + 8 * np_)
        ops.append((op, blk, pairs, adv, u16(d, pc + 2)))
        blk += adv
        pc += 8 + 8 * np_
    return ops, blk


def entries_layout(d, r):
    """Walk the instance entries from +24 the way the allocator does; return the end offset."""
    t = r['tmpl']
    e = 24
    lay = []
    if r['release']:
        lay.append(('release', e, 20)); e += 20
    for _ in range(r['nsubs']):
        lay.append(('varsub', e, 28)); e += 28
    if r['payload']:
        n = u8(d, t + e + 16)
        lay.append(('payload', e, (n + 5) * 4, n)); e += (n + 5) * 4
    for _ in range(r['nbc']):
        n = u8(d, t + e + 24)
        lay.append(('bcast', e, (n + 7) * 4, n)); e += (n + 7) * 4
    return lay, e


def main():
    parser = argparse.ArgumentParser(description='Survey every ABKC bank and MOIR project in audiofiles.big.')
    parser.add_argument('--dump', metavar='BANK.abk', help="dump one bank's records and programs")
    parser.add_argument('--disc', type=Path, default=Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')),
                        help='extracted disc root, the folder holding data/ (default: %(default)s)')
    args = parser.parse_args()
    big = BigArchive(args.disc / 'data/audio/audiofiles.big')
    csis = {}
    banks = {}
    for e in big.entries:
        n = Path(e.path).name
        if n.endswith('.csi'):
            csis[n] = Csi(n, big.read(e))
        elif n.endswith('.abk'):
            banks[n] = big.read(e)
    symbol_names = {}
    for c in csis.values():
        for t, rows in enumerate(c.tables):
            for row in rows:
                symbol_names[(t, row['id'], row['name'])] = c.name

    if args.dump:
        dump(banks[args.dump])
        return

    print(f'{len(banks)} banks, {len(csis)} csi')
    for c in csis.values():
        print(f'  {c.name:28s} project {c.project:#06x} tables {c.counts} pool@{c.pool:#x} heads0='
              f'{sum(r["head"] for t in c.tables for r in t)} gens0={sum(r["gen"] for t in c.tables for r in t)}')
    hdr = Counter(); nrec = Counter(); ops = Counter(); caps = Counter(); fails = []
    kinds = Counter(); kinds_by_suffix = defaultdict(Counter)
    curve_types = Counter(); curve_scale_one = Counter(); param_ids = Counter(); flags1517 = Counter()
    desc_counts = Counter(); end_ok = 0; first_ops = Counter(); last_ops = Counter(); entry_ok = 0
    payload_words = Counter(); triple_ok = 0; obj_after_entries = 0; voice_obj_sizes = Counter()
    adv_neg = 0; unread = Counter()
    for name, d in banks.items():
        hdr[(u32(d, 4), u16(d, 8))] += 1
        if u32(d, 0x14) != len(d): fails.append((name, 'size'))
        if u32(d, 0x18) != u32(d, 0x20): fails.append((name, 's10a twice'))
        if u32(d, 0x1C) != 0x5C: fails.append((name, 'first record'))
        if d[u32(d, 0x18):u32(d, 0x18) + 4] != b'S10A': fails.append((name, 'S10A'))
        if u32(d, u32(d, 0x30)) != 0: fails.append((name, 'code fixups non-empty'))
        nrec[u16(d, 0x0A)] += 1
        ex = u32(d, 0x38)
        for i in range(u32(d, ex)):
            tgt, rec, kind = struct.unpack_from('>III', d, ex + 4 + 12 * i)
            nm = cstr(d, rec + 4)
            kinds[kind >> 24] += 1
            suf = nm.rsplit('_', 1)[-1] if '_' in nm else nm
            kinds_by_suffix[kind >> 24][suf if suf in ('msg', 'snd', 'vol', 'gbl', 'sel') else
                                        ('c_/Class' if nm.lower().startswith(('c_', 'class')) else 'other')] += 1
        for r in records(d):
            caps[r['cap']] += 1
            if r['live'] != 0: fails.append((name, 'live nonzero on disk'))
            o, endblk = program(d, r)
            for op, blk, pairs, adv, un in o:
                ops[op] += 1
                unread[un] += 1
                if adv < 0: adv_neg += 1
            first_ops[tuple(x[0] for x in o[:3])] += 1
            last_ops[tuple(x[0] for x in o[-2:])] += 1
            if endblk == r['tsize'] - 16 + 0 or endblk == r['triple'] + 16:
                end_ok += 1
            if r['triple'] == r['tsize'] - 16:
                triple_ok += 1
            lay, eend = entries_layout(d, r)
            entry_ok += 1
            for L in lay:
                if L[0] == 'payload': payload_words[L[3]] += 1
            t = r['tmpl']
            for i, off in enumerate(r['objs'][:r['nvoice']]):
                obj = t + off
                tab = u32(d, obj + 4)
                n = u32(d, tab)
                desc_counts[n] += 1
                nrec_ = u8(d, obj + 14)
                flags1517[(u8(d, obj + 15), u8(d, obj + 17))] += 1
                for k in range(nrec_):
                    param_ids[u8(d, obj + 28 + 12 * k)] += 1
                if off < eend: obj_after_entries += 1
            for op, blk, pairs, adv, un in o:
                if op == 15:
                    c = u32(d, t + blk)
                    curve_types[u8(d, c)] += 1
                    curve_scale_one[f32(d, c + 12) == 1.0] += 1
    print('header (word 4, u16 at 8):', dict(hdr))
    print('records per bank:', dict(nrec), 'total', sum(k * v for k, v in nrec.items()))
    print('capacities:', dict(caps.most_common()))
    print('opcode census:', dict(sorted(ops.items())))
    print('unread u16 field values:', dict(unread))
    print('negative block advances:', adv_neg)
    print('program end block == triple+16:', end_ok, ' triple == tsize-16:', triple_ok)
    print('first three ops:', first_ops.most_common(5))
    print('last two ops:', last_ops.most_common(5))
    print('export kind top byte:', dict(kinds))
    for k, c in sorted(kinds_by_suffix.items()):
        print(f'  kind {k}:', dict(c.most_common()))
    print('payload word counts:', dict(sorted(payload_words.items())))
    print('voice objects: descriptor counts', dict(sorted(desc_counts.items())))
    print('voice objects: (+15 time copy-back, +17 extra copy-back):', dict(flags1517))
    print('voice param ids:', dict(sorted(param_ids.items())))
    print('voice objects placed inside the entry area (should be 0):', obj_after_entries)
    print('curve sample types:', dict(curve_types), ' scale==1.0 (nearest):', dict(curve_scale_one))
    print('failures:', fails[:20], len(fails))


def dump(d):
    for r in records(d):
        print({k: (hex(v) if isinstance(v, int) else v) for k, v in r.items()})
        print(' entries:', entries_layout(d, r))
        o, e = program(d, r)
        for op, blk, pairs, adv, un in o:
            ps = ', '.join(f'R->{blk + dst}' if src == -1 else f'{blk + src}->{blk + dst}' for src, dst in pairs)
            print(f'  op{op:<2} blk {blk:<5} [{ps}] adv {adv}')


if __name__ == '__main__':
    main()
