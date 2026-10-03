"""Simulate a distance prefetch of world emitter banks over a play session's audio state log (board
position as the listener) and report resident memory:
  loaded  = banks loaded into the runtime (on first start, kept until map change);
  pending = banks prefetched (decoded) but not loaded yet = the extra memory of the prefetch.
Reach is approximated by the record's largest extent (the game's ellipsoid test is tighter, so this
over-counts loaded banks slightly).

usage: py -3.13 tools/audio-bench/prefetch_memory_sim.py STATE_LOG EMS[,EMS...] [--ahead M] [--evict M] [--assets DIR]
  STATE_LOG: a log written with SKATE_AUDIO_STATE_LOG=<path> (needs board_x/y/z columns).
  EMS: the map's .ems emitter files, as named in the manifest (e.g. sfx_university,music_university).
"""
import argparse
import csv
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emitter_bank_memory as ebm  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description='Simulate a distance prefetch of world emitter banks over a state log.')
    parser.add_argument('log', help='SKATE_AUDIO_STATE_LOG file')
    parser.add_argument('ems', help='comma-separated .ems files of the map')
    parser.add_argument('--ahead', type=float, default=60.0, help='prefetch distance beyond reach, m (default: %(default)s)')
    parser.add_argument('--evict', type=float, default=90.0, help='drop distance beyond reach, m (default: %(default)s)')
    parser.add_argument('--assets', default=os.environ.get('SKATE_ASSETS', 'assets'), help='asset root (default: %(default)s)')
    args = parser.parse_args()
    ebm.load(args.assets)
    recs = ebm.records([f for f in args.ems.split(',') if f])
    loaded, pending = set(), set()
    peak_pending = peak_total = 0.0
    rows = 0

    def mib(s):
        return sum(ebm.bank_size(b) for b in s) / 2**20
    with open(args.log, newline='') as f:
        for row in csv.DictReader(f, delimiter='\t'):
            try:
                p = (float(row['board_x']), float(row['board_y']), float(row['board_z']))
            except (KeyError, ValueError, TypeError):
                continue
            rows += 1
            want, keep = set(), set()
            for r in recs:
                d = math.dist(p, r['position'])
                radius = max(r['extent'])
                if d <= radius:
                    loaded.add(r['bank'])
                if d <= radius + args.ahead:
                    want.add(r['bank'])
                if d <= radius + args.evict:
                    keep.add(r['bank'])
            pending = {b for b in (pending | want) if b not in loaded and b in keep}
            peak_pending = max(peak_pending, mib(pending))
            peak_total = max(peak_total, mib(pending | loaded))
    print(f'{os.path.basename(args.log)} rows {rows} ahead {args.ahead:g} m evict {args.evict:g} m: loaded {len(loaded)} banks '
          f'{mib(loaded):.1f} MiB; prefetched-not-loaded peak {peak_pending:.1f} MiB; loaded+prefetched peak {peak_total:.1f} MiB')


if __name__ == '__main__':
    main()
