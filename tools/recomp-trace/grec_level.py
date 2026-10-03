"""Retail grain-bed levels from GREC lines (the grain-bed owner hook, local player, once per update).

usage: py -3.13 tools/recomp-trace/grec_level.py <trace.tsv> [--turn-max 0.02]
GREC fields (tab-separated): owner | truck0 "A: gain pitch pos active | B: gain pitch pos active | running" |
truck1 (same) | owner floats +1160 I(turn intensity) +1164 +1168 Bk(brake slew) +1508 downhill +1456 +1464
+1028 +1032 +1152 +1156 | state: +204 turn input, +208 ground speed, +200 wheels, +332 air (byte),
+336 brake (hex word), +340 balance (hex word), +620 material.
Straight roll = |turn input| <= --turn-max, air 0, wheels > 0, truck running, I and Bk ~ 0. For each speed band
(km/h) prints: frames, gain A / gain B / pitch A of the running truck (median, p10, p90), and
level(1) = gain A / (1 - max(|I|, Bk)) — the quantity the native port's bed gain A must match. Also a carve
section (|turn input| > 0.3): B vs turn and A dip, and a manual check is left to the caller.
"""
import sys
from collections import defaultdict


def q(values, p):
    v = sorted(values)
    return v[min(len(v) - 1, int(len(v) * p))] if v else float('nan')


def parse(line):
    f = line.rstrip('\r\n').split('\t')
    if len(f) < 7 or f[0] != 'GREC':
        return None
    trucks = []
    for t in (f[3], f[4]):
        parts = [p.strip() for p in t.split('|')]
        if len(parts) != 3:
            return None
        a, b = parts[0].split(), parts[1].split()
        if len(a) != 4 or len(b) != 4 or '-' in a[0]:
            return None
        trucks.append(dict(a_gain=float(a[0]), a_pitch=float(a[1]), a_on=int(a[3]), b_gain=float(b[0]),
                           b_pitch=float(b[1]), b_on=int(b[3]), running=int(parts[2])))
    o = [float(x) for x in f[5].split()]
    s = f[6].split()
    if len(o) != 10 or len(s) != 7:
        return None
    return dict(ms=float(f[1]), trucks=trucks, I=o[0], Bk=o[2], downhill=o[3], turn=float(s[0]), speed=float(s[1]),
                wheels=int(s[2]), air=int(s[3]))


def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    path = sys.argv[1]
    turn_max = float(sys.argv[sys.argv.index('--turn-max') + 1]) if '--turn-max' in sys.argv else 0.02
    straight = defaultdict(lambda: defaultdict(list))
    carve = defaultdict(list)
    n = bad = 0
    for raw in open(path, encoding='utf-8', errors='replace'):
        if not raw.startswith('GREC\t'):
            continue
        r = parse(raw)
        if r is None:
            bad += 1
            continue
        n += 1
        running = [t for t in r['trucks'] if t['running']]
        if not running or r['air'] or r['wheels'] == 0:
            continue
        t = running[0]
        kmh = r['speed'] * 3.6
        band = int(kmh // 5) * 5
        if abs(r['turn']) <= turn_max and abs(r['I']) < 0.02 and r['Bk'] < 0.02:
            d = straight[band]
            d['a'].append(t['a_gain'])
            d['b'].append(t['b_gain'])
            d['pitch'].append(t['a_pitch'])
            d['level'].append(t['a_gain'] / max(1e-6, 1 - max(abs(r['I']), r['Bk'])))
        elif abs(r['turn']) > 0.3:
            carve[band].append((abs(r['turn']), abs(r['I']), t['a_gain'], t['b_gain']))
    print(f'GREC lines parsed {n}, unparsable {bad}')
    print('STRAIGHT ROLL (|turn| <= %.2f, I, Bk ~ 0)' % turn_max)
    print('  km/h   frames   gainA med [p10 p90]      gainB med     pitchA med    level(1) med [p10 p90]')
    for band in sorted(straight):
        d = straight[band]
        if len(d['a']) < 30:
            continue
        print(f"  {band:3d}-{band + 5:<3d} {len(d['a']):7d}   {q(d['a'], .5):.3f} [{q(d['a'], .1):.3f} {q(d['a'], .9):.3f}]"
              f"   {q(d['b'], .5):.3f}       {q(d['pitch'], .5):.3f}        {q(d['level'], .5):.3f} [{q(d['level'], .1):.3f} {q(d['level'], .9):.3f}]")
    print('CARVE (|turn| > 0.3)')
    print('  km/h   frames   |turn| med   I med   gainA med   gainB med')
    for band in sorted(carve):
        rows = carve[band]
        if len(rows) < 30:
            continue
        print(f"  {band:3d}-{band + 5:<3d} {len(rows):7d}   {q([x[0] for x in rows], .5):.2f}       {q([x[1] for x in rows], .5):.2f}"
              f"    {q([x[2] for x in rows], .5):.3f}       {q([x[3] for x in rows], .5):.3f}")


if __name__ == '__main__':
    main()
