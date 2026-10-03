"""Retail grain-bed gain A on CLEAN straight rolling from GREC lines (see grec_level.py for the fields).

usage: py -3.13 tools/recomp-trace/grec_clean.py <trace.tsv> [--settle FRAMES] [--material M]
Clean = four wheels down, not in the air, |turn input| <= 0.02, I and Bk ~ 0, the +336 word 0 (no brake /
push event / manual brake) and the +340 word 0 (no balance, grind, trick), held for --settle consecutive
GREC lines (default 30). Prints per speed band the gain A of every running truck (median, p10, p90), the
two trucks' gains when both run, and the owner floats +1456 / +1028 (unknown roles) so a per-truck factor
shows up.
"""
import sys
from collections import defaultdict

sys.path.insert(0, __import__('os').path.dirname(__file__))
from grec_level import parse, q  # noqa: E402


def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    path = sys.argv[1]
    settle = int(sys.argv[sys.argv.index('--settle') + 1]) if '--settle' in sys.argv else 30
    only = int(sys.argv[sys.argv.index('--material') + 1]) if '--material' in sys.argv else None
    bands = defaultdict(lambda: defaultdict(list))
    run = 0
    for raw in open(path, encoding='utf-8', errors='replace'):
        if not raw.startswith('GREC\t'):
            continue
        r = parse(raw)
        if r is None:
            continue
        s = raw.rstrip('\r\n').split('\t')[6].split()
        o = [float(x) for x in raw.rstrip('\r\n').split('\t')[5].split()]
        clean = (r['wheels'] == 4 and not r['air'] and abs(r['turn']) <= 0.02 and abs(r['I']) < 0.02 and r['Bk'] < 0.02
                 and s[4] == '00000000' and s[5] == '00000000' and (only is None or int(s[6]) == only))
        run = run + 1 if clean else 0
        if run < settle:
            continue
        band = int(r['speed'] * 3.6 // 5) * 5
        d = bands[band]
        running = [t for t in r['trucks'] if t['running']]
        for t in running:
            d['a'].append(t['a_gain'])
        if len(running) == 2:
            d['both'].append((running[0]['a_gain'], running[1]['a_gain']))
        d['o1456'].append(o[4])
        d['o1028'].append(o[6])
        d['mat'].append(int(s[6]))
    print(f'CLEAN STRAIGHT ROLL (settled {settle} lines)')
    print('  km/h   n      gainA med [p10 p90]    both-running n  t0/t1 med   +1456 med  +1028 med  materials')
    for band in sorted(bands):
        d = bands[band]
        if len(d['a']) < 20:
            continue
        both = d['both']
        mats = defaultdict(int)
        for m in d['mat']:
            mats[m] += 1
        top = ' '.join(f'{m}:{c}' for m, c in sorted(mats.items(), key=lambda x: -x[1])[:4])
        print(f"  {band:3d}-{band + 5:<3d} {len(d['a']):6d}  {q(d['a'], .5):.3f} [{q(d['a'], .1):.3f} {q(d['a'], .9):.3f}]"
              f"   {len(both):6d}  {q([a for a, _ in both], .5) if both else float('nan'):.3f}/{q([b for _, b in both], .5) if both else float('nan'):.3f}"
              f"   {q(d['o1456'], .5):.3f}      {q(d['o1028'], .5):.3f}     {top}")


if __name__ == '__main__':
    main()
