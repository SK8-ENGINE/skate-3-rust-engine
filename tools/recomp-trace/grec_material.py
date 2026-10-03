"""Retail wheel-0 material (+620) vs wheel count (+200) from GREC lines: does the material go to the
"no contact" value when a wheel loses contact, and how often does it change while rolling? (Class_Seams
fires a hit on every material change.)
usage: py -3.13 tools/recomp-trace/grec_material.py SESSION_DIR [SESSION_DIR...] [--no-contact N]
  --no-contact: the material value logged without ground contact (default 143, as seen in the traces)."""
import collections
import sys
from pathlib import Path

if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
    sys.exit(__doc__)
args = sys.argv[1:]
NO_CONTACT = 143
if '--no-contact' in args:
    i = args.index('--no-contact')
    NO_CONTACT = int(args[i + 1])
    del args[i:i + 2]
for d in args:
    rows = []
    for line in (Path(d) / 'trace.tsv').open(errors='replace'):
        if not line.startswith('GREC\t'):
            continue
        f = line.rstrip('\n').split('\t')
        if len(f) < 7:
            continue
        s = f[6].split()
        if len(s) != 7:
            continue
        try:
            rows.append((float(f[1]), float(s[1]), int(s[2]), int(s[3]), int(s[6])))
        except ValueError:
            continue
    # The hook can log several calls per game frame (a few ms apart): keep one row per frame.
    kept, last = [], -1e9
    for r in rows:
        if r[0] - last >= 10:
            kept.append(r)
            last = r[0]
    rows = kept
    by = collections.Counter()
    changes = moving = 0
    prev = None
    for ms, v, wheels, air, mat in rows:
        if abs(v) > 2 and not air:
            moving += 1
            by[(wheels, mat == NO_CONTACT)] += 1
            if prev is not None and prev[0] > ms - 40 and mat != prev[1]:
                changes += 1
        prev = (ms, mat)
    print(d, f'{moving / 60:.0f} s rolling (>2 m/s, not air)')
    if not moving:
        continue
    for w in range(5):
        a, b = by[(w, False)], by[(w, True)]
        if a + b:
            print(f'  wheels {w}: {a + b} frames, material {NO_CONTACT} in {b} ({100 * b / (a + b):.0f} %)')
    print(f'  wheel-0 material changes: {changes / (moving / 60):.2f} /s')
