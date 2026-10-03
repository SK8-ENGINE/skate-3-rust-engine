"""Per-frame voice gains by bank from an e2e render's <name>.ours.voices.tsv.
usage: py -3.13 voices_summary.py VOICES_TSV [FROM TO STEP]  (frames of the scenario)."""
import csv
import sys
if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
    sys.exit(__doc__)
from collections import defaultdict

rows = list(csv.DictReader(open(sys.argv[1]), delimiter='\t'))
a, b, step = (int(x) for x in sys.argv[2:5]) if len(sys.argv) > 4 else (0, 10 ** 9, 6)
by = defaultdict(lambda: defaultdict(float))
banks = set()
for r in rows:
    f = int(r['frame'])
    key = r['bank'] if not r['bank'].startswith('splice:') else 'splice'
    key = 'grain' if r['slot'].startswith('grain') else key
    by[f][key] += float(r['gain'])
    banks.add(key)
banks = sorted(banks)
print('frame ' + ' '.join(f'{k[:14]:>14}' for k in banks))
for f in range(a, min(b, max(by) + 1 if by else 0), step):
    print(f'{f:5d} ' + ' '.join(f'{by[f].get(k, 0):14.4f}' for k in banks))
