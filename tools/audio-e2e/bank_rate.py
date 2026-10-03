"""Voice starts per second and summed gain of one bank in e2e renders, per (tag, seam pattern) of the
scenario, for before/after comparisons (e.g. Class_Seams on a surface).
usage: py -3.13 bank_rate.py BANK DIR NAME [DIR ...]   (a voice start = a frame where the bank has more
voices than in the frame before, counted by the increase)"""
import collections
import csv
import sys
if len(sys.argv) < 4 or sys.argv[1] in ('-h', '--help'):
    sys.exit(__doc__)

bank, name, dirs = sys.argv[1], sys.argv[3], [sys.argv[2]] + sys.argv[4:]
for d in dirs:
    rows = list(csv.DictReader(open(f'{d}/{name}.tsv'), delimiter='\t'))
    count = collections.Counter()
    gain = collections.defaultdict(float)
    for r in csv.reader(open(f'{d}/{name}.ours.voices.tsv'), delimiter='\t'):
        if r[0] != 'frame' and r[1] == bank:
            count[int(r[0])] += 1
            gain[int(r[0])] += float(r[3])
    agg = collections.defaultdict(lambda: [0, 0, 0.0])
    for f, r in enumerate(rows):
        if float(r['speed']) < 2 or r['state'] != '100':
            continue
        k = (r['tag'], r.get('seam0', '?'))
        a = agg[k]
        a[0] += 1
        a[1] += max(0, count[f] - count[f - 1]) if f else count[f]
        a[2] += gain[f]
    print(d)
    for k, (n, s, g) in sorted(agg.items(), key=lambda x: -x[1][0]):
        if n >= 30:
            print(f'  tag {k[0]:>3} pattern {k[1]:>2}: {n / 60:5.1f} s  starts {s / (n / 60):5.1f}/s  mean summed gain {g / n:.4f}')
