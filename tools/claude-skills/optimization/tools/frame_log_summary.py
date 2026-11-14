"""Summarise a SKATE_FRAME_LOG file (frame log v1).

  py -3.13 frame_log_summary.py LOG.tsv [--hitches N]

Prints frame-time percentiles, the per-second worst frame (median / p90 / max),
how often physics had to catch up (fixed_steps > 1), the main-thread CPU share,
and the N longest hitches with their UTC wall time, to line up with
logs\\game-*.stderr.log and the audio state log. Rejects the whole file on a
malformed row (bad data is deleted, not parsed around).
"""
import argparse, datetime, math, sys
from collections import defaultdict

MAGIC = '# skate3rust frame log v1'
COLUMNS = ['wall_unix_s', 'frame', 'frame_ms', 'fixed_steps', 'fixed_ms', 'main_ms', 'hitch', 'median_ms']


def pct(sorted_values, p):
    if not sorted_values:
        return 0.0
    rank = math.ceil(p * len(sorted_values) / 100 - 1e-9)
    return sorted_values[min(max(rank, 1), len(sorted_values)) - 1]


def load(path):
    rows = []
    with open(path, encoding='utf-8') as f:
        if f.readline().rstrip('\n') != MAGIC or f.readline().rstrip('\n').split('\t') != COLUMNS:
            sys.exit(f'{path}: not a frame log v1')
        for number, line in enumerate(f, 3):
            fields = line.rstrip('\n').split('\t')
            if len(fields) != len(COLUMNS) or fields[6] not in ('', 'HITCH'):
                sys.exit(f'{path}:{number}: malformed row; delete the file')
            rows.append(dict(wall=float(fields[0]), frame=int(fields[1]), ms=float(fields[2]),
                             steps=int(fields[3]), fixed_ms=float(fields[4]), main_ms=float(fields[5]),
                             hitch=fields[6] == 'HITCH', median=float(fields[7]) if fields[7] else None))
    return rows


def main():
    p = argparse.ArgumentParser()
    p.add_argument('log')
    p.add_argument('--hitches', type=int, default=15)
    a = p.parse_args()
    rows = load(a.log)
    if not rows:
        sys.exit('no frames')
    ms = sorted(r['ms'] for r in rows)
    span = rows[-1]['wall'] - rows[0]['wall']
    print(f'{len(rows)} frames over {span:.1f} s ({len(rows) / max(span, 1e-9):.1f} fps mean)')
    print('frame ms: ' + '  '.join(f'p{q}={pct(ms, q):.2f}' for q in (50, 90, 99, 99.9)) + f'  max={ms[-1]:.1f}')
    per_second = defaultdict(float)
    for r in rows:
        per_second[int(r['wall'])] = max(per_second[int(r['wall'])], r['ms'])
    worst = sorted(per_second.values())
    print(f'per-second worst frame: median {pct(worst, 50):.1f}  p90 {pct(worst, 90):.1f}  max {worst[-1]:.1f} ms; '
          f'seconds with a frame > 50 ms: {sum(v > 50 for v in worst)}')
    catch_up = sum(r['steps'] > 1 for r in rows)
    print(f'frames with physics catch-up (fixed_steps > 1): {catch_up} ({100 * catch_up / len(rows):.1f} %)')
    main = sorted(r['main_ms'] for r in rows)
    print(f'main-thread CPU ms: p50 {pct(main, 50):.2f}  p99 {pct(main, 99):.2f}; '
          f'fixed (physics) ms p50 {pct(sorted(r["fixed_ms"] for r in rows), 50):.2f}')
    hitches = sorted((r for r in rows if r['hitch']), key=lambda r: -r['ms'])
    print(f'{len(hitches)} hitches (> 2x median); longest:')
    for r in hitches[:a.hitches]:
        when = datetime.datetime.fromtimestamp(r['wall'], datetime.timezone.utc).strftime('%H:%M:%S.%f')[:-3]
        bound = 'main thread' if r['main_ms'] > 0.5 * r['ms'] else 'render/GPU/present or OS'
        print(f'  {when} UTC frame {r["frame"]:>7}  {r["ms"]:7.1f} ms (median {r["median"]:.1f})  '
              f'main {r["main_ms"]:.1f} ms, physics {r["fixed_ms"]:.1f} ms x{r["steps"]} -> {bound}')


if __name__ == '__main__':
    main()
