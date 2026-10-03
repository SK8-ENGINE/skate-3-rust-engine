"""List every collision surface height at a spawn's XZ (orientation ignored).

Usage (repo root, after map_collision.py):
    py -3.13 tools/collision-inspect/analyse_spawn.py MegaPark 21.742 21.605 -19.683 [--work DIR]
Output: (height, normal-y sign, area m2) for each surface under/over the point, plus the
area-weighted centre of flat surfaces. Nothing below the spawn height = void; a much
lower surface under it = roof/platform.
"""
import argparse
from pathlib import Path
import numpy as np

REPO = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description="List every collision surface height at a point's XZ.")
parser.add_argument('district', help='district name without DIST_')
parser.add_argument('x', type=float)
parser.add_argument('y', type=float, help='ignored (every height at x, z is listed)')
parser.add_argument('z', type=float)
parser.add_argument('--work', type=Path, default=REPO / '.local/collision', help='work folder (default: %(default)s)')
args = parser.parse_args()
name, x, z = args.district, args.x, args.z
t = np.load(args.work / name / 'collision.npy')
a, b, c = t[:, 0], t[:, 1], t[:, 2]
ab, ac = b - a, c - a
cross = np.cross(ab, ac)
area = np.linalg.norm(cross, axis=1) / 2
up = cross[:, 1] / np.maximum(2 * area, 1e-9)

def surfaces(px, pz):
    dx, dz = px - a[:, 0], pz - a[:, 2]
    det = ab[:, 0] * ac[:, 2] - ac[:, 0] * ab[:, 2]
    ok = np.abs(det) > 1e-12
    d = np.where(ok, det, 1.0)
    u = (dx * ac[:, 2] - ac[:, 0] * dz) / d
    v = (ab[:, 0] * dz - dx * ab[:, 2]) / d
    inside = ok & (u >= -1e-5) & (v >= -1e-5) & (u + v <= 1.00001)
    height = a[:, 1] + u * ab[:, 1] + v * ac[:, 1]
    return sorted({(round(float(height[i]), 2), round(float(up[i]), 2), round(float(area[i]), 1))
                   for i in np.nonzero(inside)[0]})

print(name, 'surfaces at', (x, z), ':', surfaces(x, z))
centre = (a + b + c) / 3
flat = (np.abs(up) >= .9) & (area >= 2)
w = area[flat]
cx = float((centre[flat, 0] * w).sum() / w.sum())
cz = float((centre[flat, 2] * w).sum() / w.sum())
print('flat-area centre', (round(cx, 1), round(cz, 1)), ':', surfaces(cx, cz)[:6])
