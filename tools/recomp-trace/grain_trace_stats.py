"""Grain voice starts in a recomp audio trace (PLAY lines whose sample header matches a .grain stream of
your own disc's grains.big).

usage: py -3.13 tools/recomp-trace/grain_trace_stats.py <trace.tsv> [--disc DIR] [--exclude MEMBER,...] [--out JSON]
  --disc     extracted disc root, the folder holding data/ (default $SKATE3_DISC or .local/skate3-disc)
  --exclude  members left out of the pooled interval histogram (e.g. a non-board grain)
  --out      JSON output (default .local/recomp-trace/grain_stats_<session>.json)
Prints per-member start counts, starts per second in 1 s windows (histogram) and the inter-start
interval histogram (all grain members pooled, and per member).
"""
import argparse
import collections
import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.owned_game.big import BigArchive  # noqa: E402


def hist(ts):
    h = collections.Counter()
    for a, b in zip(ts, ts[1:]):
        h[min(int((b - a) // 25) * 25, 1000)] += 1
    return dict(sorted(h.items()))


def main():
    parser = argparse.ArgumentParser(description='Grain voice starts in a recomp audio trace.')
    parser.add_argument('trace', type=Path)
    parser.add_argument('--disc', type=Path, default=Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')))
    parser.add_argument('--exclude', default='', help='comma-separated members left out of the pooled histogram')
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    exclude = {m for m in args.exclude.split(',') if m}
    big = BigArchive(args.disc / 'data/audio/grains.big')
    pref = {}
    for e in big.entries:
        b = big.read(e)
        h = int.from_bytes(b[:4], 'big')
        pref[b[h:h + 12].hex()] = Path(e.path).name
    starts = []  # (ms, member, player, level)
    for line in args.trace.open(encoding='utf-8', errors='replace'):
        if not line.startswith('PLAY'):
            continue
        f = line.rstrip('\n').split('\t')
        m = pref.get(f[-1][:24])
        if m and len(f) > 5:
            starts.append((float(f[1]), m, f[2], f[5]))
    per = collections.Counter(s[1] for s in starts)
    sec = collections.Counter(int(s[0] // 1000) for s in starts)
    out = {'trace': str(args.trace), 'starts': len(starts), 'per_member': dict(per),
           'starts_per_second_hist': dict(sorted(collections.Counter(sec.values()).items())),
           'interval_ms_hist_pooled': hist([s[0] for s in starts if s[1] not in exclude]),
           'interval_ms_hist_per_member': {m: hist([s[0] for s in starts if s[1] == m]) for m in per},
           'levels_sample': collections.Counter(s[3] for s in starts).most_common(8)}
    dst = args.out or REPO / '.local/recomp-trace' / f'grain_stats_{args.trace.parent.name}.json'
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text(json.dumps(out, indent=1))
    print(json.dumps({k: out[k] for k in ('starts', 'per_member', 'starts_per_second_hist', 'interval_ms_hist_pooled',
                                          'levels_sample')}, indent=0))
    for m, _ in per.most_common(3):
        print(m, out['interval_ms_hist_per_member'][m])


if __name__ == '__main__':
    main()
