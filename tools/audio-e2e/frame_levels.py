"""Per-frame level of an e2e render next to the scenario's state: the folded stereo RMS (dBFS, the
title's 0.707/0.5/0.5 fold) and the summed per-voice gain of each voice group (grain bed, wheel
spin, splice one-shots, each AEMS bank), so you can see which voice is audible when.
usage: py -3.13 frame_levels.py DIR NAME [FROM TO STEP]   (frames of the scenario; default all, step 3)
Reads DIR/NAME.tsv (scenario), DIR/NAME.ours.f32 (6 ch: L R C LFE Ls Rs), DIR/NAME.ours.voices.tsv."""
import csv
import math
import struct
import sys
if len(sys.argv) < 3 or sys.argv[1] in ('-h', '--help'):
    sys.exit(__doc__)
from collections import defaultdict
from pathlib import Path

d, name = Path(sys.argv[1]), sys.argv[2]
a, b, step = (int(x) for x in sys.argv[3:6]) if len(sys.argv) > 5 else (0, 10 ** 9, 3)
rows = list(csv.DictReader(open(d / f'{name}.tsv'), delimiter='\t'))
raw = (d / f'{name}.ours.f32').read_bytes()
n = len(raw) // 24
pcm = struct.unpack(f'<{n * 6}f', raw[:n * 24])
SETTLE = 60
spf = 800  # 48000 / 60
voices = defaultdict(lambda: defaultdict(float))
groups = set()
for r in csv.DictReader(open(d / f'{name}.ours.voices.tsv'), delimiter='\t'):
    bank = r['bank']
    key = 'grain' if bank.startswith('grain') or bank == 'rocket' else ('splice' if bank.startswith('splice:') else bank)
    key = 'direct' if key == '?' else key  # Splice / stream voices as the mixer lists them
    voices[int(r['frame'])][key] += float(r['gain'])
    groups.add(key)
groups = sorted(groups)
print('frame  state air wh   speed  dBFS   ' + ' '.join(f'{g[:10]:>10}' for g in groups))
for f in range(a, min(b, len(rows)), step):
    s0 = (SETTLE + f) * spf
    acc = 0.0
    for k in range(s0, min(s0 + spf, n)):
        L, R, C, _, Ls, Rs = pcm[6 * k:6 * k + 6]
        lo = 0.707 * L + 0.5 * C + 0.5 * Ls
        ro = 0.707 * R + 0.5 * C + 0.5 * Rs
        acc += lo * lo + ro * ro
    rms = math.sqrt(acc / (2 * spf)) if acc > 0 else 0.0
    db = 20 * math.log10(rms) if rms > 0 else -120.0
    r = rows[f]
    print(f"{f:5d} {r['state']:>5} {r['air']:>3} {r['wheels']:>3} {float(r['speed']):6.2f} {db:6.1f}   "
          + ' '.join(f'{voices[f].get(g, 0):10.4f}' for g in groups))
