"""Fuses the posed collision of the layout pieces around a point (vertex merge within `weld`, as skate_world.rs
portable_parts_fused) and reports every edge near the point that more than two triangles share, with the triangles'
normals: a seam where two pieces' side walls meet under their tops (doc 39, USER-ASKS 221).

usage: python park-seam-edges.py <layout.json> x y z [radius_m] [weld_m]
"""
import json
import math
import sys
from collections import defaultdict

layout = json.load(open(sys.argv[1]))
point = [float(v) for v in sys.argv[2:5]]
radius = float(sys.argv[5]) if len(sys.argv) > 5 else 0.6
weld = float(sys.argv[6]) if len(sys.argv) > 6 else 0.01

tris = []
for inst in layout['instances']:
    col = layout['collision'].get(inst['template_id'])
    if not col:
        continue
    m = inst['matrix']
    pose = lambda v: tuple(sum(v[k] * m[k][j] for k in range(3)) + m[3][j] for j in range(3))
    for tri in col['triangles']:
        posed = [pose(v) for v in tri]
        if any(math.dist(p, point) < radius + 3 for p in posed):
            tris.append((inst['name'], posed))

key = lambda p: tuple(round(c / weld) for c in p)
edges = defaultdict(list)
for i, (_, t) in enumerate(tris):
    ids = [key(p) for p in t]
    if len(set(ids)) < 3:
        continue
    for a, b in ((0, 1), (1, 2), (2, 0)):
        edges[tuple(sorted((ids[a], ids[b])))].append(i)


def normal(t):
    u = [t[1][j] - t[0][j] for j in range(3)]
    v = [t[2][j] - t[0][j] for j in range(3)]
    n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    s = math.sqrt(sum(c * c for c in n)) or 1.0
    return [round(c / s, 2) for c in n]


shared = defaultdict(int)
for edge, users in edges.items():
    shared[len(users)] += 1
    a, b = [[c * weld for c in p] for p in edge]
    mid = [(a[j] + b[j]) / 2 for j in range(3)]
    if len(users) > 2 and math.dist(mid, point) < radius:
        print(f"edge ({a[0]:.2f}, {a[1]:.3f}, {a[2]:.2f})-({b[0]:.2f}, {b[1]:.3f}, {b[2]:.2f}) shared by {len(users)}: "
              + '; '.join(f"{tris[i][0]} n{normal(tris[i][1])}" for i in users))
print('triangles per edge in the area:', dict(sorted(shared.items())))
