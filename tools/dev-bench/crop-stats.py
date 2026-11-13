# Dev only: colour statistics of a rectangle in a capture (water look matching, doc 09).
# Usage: python -I crop-stats.py <png> <x0> <y0> <x1> <y1>
import sys

import numpy as np
from PIL import Image

path, x0, y0, x1, y1 = sys.argv[1], *map(int, sys.argv[2:6])
px = np.asarray(Image.open(path).convert('RGB'), dtype=np.float64)[y0:y1, x0:x1]
mean = px.reshape(-1, 3).mean(0)
luma = px @ [0.299, 0.587, 0.114]
p5, p50, p95 = np.percentile(luma, [5, 50, 95])
print(f'{path}: mean sRGB {mean.round(0).astype(int).tolist()} luma p5/p50/p95 {p5:.0f}/{p50:.0f}/{p95:.0f}'
      f' blue-red {mean[2] - mean[0]:.0f} luma stdev {luma.std():.1f}')
