"""Compare two directories of e2e renders (`<name>.ours.f32`, interleaved 6-channel f32 at 48 kHz):
per scenario bit-identical / max |difference| / RMS level of each (dBFS over the six channels) and
the level change. Headless; reads files only.

usage: py -3.13 tools/audio-e2e/render_diff.py DIR_A DIR_B
"""
import math
import sys
from pathlib import Path

import numpy as np


def rms_db(x):
    r = float(np.sqrt(np.mean(np.square(x, dtype=np.float64)))) if x.size else 0.0
    return 20 * math.log10(r) if r > 0 else -200.0


def main():
    if len(sys.argv) != 3 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    a, b = Path(sys.argv[1]), Path(sys.argv[2])
    print(f"{'scenario':14} {'same':>5} {'max|d|':>10} {'A dBFS':>8} {'B dBFS':>8} {'B-A dB':>7}")
    for fa in sorted(a.glob('*.ours.f32')):
        fb = b / fa.name
        if not fb.exists():
            continue
        xa = np.fromfile(fa, dtype='<f4')
        xb = np.fromfile(fb, dtype='<f4')
        n = min(xa.size, xb.size)
        same = xa.size == xb.size and xa.tobytes() == xb.tobytes()
        d = float(np.max(np.abs(xa[:n] - xb[:n]))) if n else 0.0
        la, lb = rms_db(xa), rms_db(xb)
        print(f"{fa.name.split('.')[0]:14} {str(same):>5} {d:10.3g} {la:8.2f} {lb:8.2f} {lb - la:+7.2f}")


if __name__ == '__main__':
    main()
