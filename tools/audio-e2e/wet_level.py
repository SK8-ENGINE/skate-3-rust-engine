"""The bus (wet) contribution of an e2e render: RMS of (wet render − dry render) against the dry RMS, over
the scenario's rolling frames, folded to stereo with the title's 0.707/0.5/0.5 table.
usage: py -3.13 wet_level.py DRY_DIR WET_DIR NAME [WET_DIR ...]"""
import csv
import math
import struct
import sys
if len(sys.argv) < 4 or sys.argv[1] in ('-h', '--help'):
    sys.exit(__doc__)

dry_dir, name, wets = sys.argv[1], sys.argv[3], [sys.argv[2]] + sys.argv[4:]


def load(d):
    raw = open(f'{d}/{name}.ours.f32', 'rb').read()
    return struct.unpack(f'<{len(raw) // 4}f', raw)


rows = list(csv.DictReader(open(f'{dry_dir}/{name}.tsv'), delimiter='\t'))
dry = load(dry_dir)
SETTLE, SPF = 60, 800
frames = [f for f, r in enumerate(rows) if float(r['speed']) > 2 and r['state'] == '100']


def fold(p, k):
    L, R, C, _, Ls, Rs = p[6 * k:6 * k + 6]
    return 0.707 * L + 0.5 * C + 0.5 * Ls, 0.707 * R + 0.5 * C + 0.5 * Rs


for d in wets:
    wet = load(d)
    n = min(len(wet), len(dry)) // 6
    ed = ew = 0.0
    for f in frames:
        for k in range((SETTLE + f) * SPF, min((SETTLE + f + 1) * SPF, n)):
            dl, dr = fold(dry, k)
            wl, wr = fold(wet, k)
            ed += dl * dl + dr * dr
            ew += (wl - dl) ** 2 + (wr - dr) ** 2
    print(f'{d}: dry {10 * math.log10(ed / (2 * SPF * len(frames))):.1f} dBFS, bus part {10 * math.log10(ew / ed):+.1f} dB re dry')
