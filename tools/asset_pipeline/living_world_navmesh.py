"""NavPower navmesh (stream object ``0x00EB0027``) -> ``living_world/navmesh.bin`` (peds milestone M3).

Retail pedestrians navigate on NavPower nav graphs (BabelFlux NavPower middleware; one graph per ``cSim`` tile).
Layout read from the user's disc (all 678 objects of the three city districts; 63283 polygons, every neighbour
reference resolves) [data]:

- Header (big-endian): a NavPower object header, then the nav graph at ``base``: f32 x4 agent parameters
  (0.12 / 0.35 / 0.2 / 1.6, most likely cell size, agent radius, step height, agent height; unverified), f32 x6
  tile bounds (min xyz, max xyz). ``base`` is 0x80 in most objects (also 0xD0, 0x120, 0x170, 0x1C0, 0x210).
- Polygons follow, each: f32 x3 centroid, f32 radius, f32 FLT_MAX, u32 0, u32 flags A (``0x007FFFxx``), u32 flags B
  (bits 8..15 = the area byte: 0x11 pavement / default, 0xA1 road carriageway, 0xF1 other; bit 26 also seen), u32 C
  (top byte small, meaning open), then the edge records, 24 bytes each: u32 neighbour (offset of the polygon across
  the edge from ``v[k]`` to ``v[k + 1]``, relative to ``base``; 0 = none), f32 x3 vertex ``v[k]``, u16 0xFFFF, u16
  edge flags (0x2000 seen), u32 0. The record list ends at the first record without the 0xFFFF marker; 4-12 bytes
  of padding precede the next polygon.
- Area evidence [data + trace]: 98 % of DownTown road piece centres lie on 0xA1 polygons; recomp ped positions
  (sessions ``all_20261004_180303`` / ``all_20261002_164620``, PEDXYZ) lie on 0x11 polygons 91-92 %, on 0xA1 3-4 %,
  never on 0xF1.

Cross-tile edges carry no neighbour in the data (tiles stop about 0.14 m short of their 100 m cell borders); the
export links a boundary edge within 0.5 m of its tile's bounds to the polygon of another tile found 0.5 m beyond its
midpoint (the runtime joins tiles itself; how is not read).

``navmesh.bin`` (little-endian): ``LWNAVMSH``, u32 version, u32 district count; per district: u8 name length +
ASCII, f32 x4 agent parameters, u32 vertex count + f32 x3 each, u32 polygon count, per polygon: u32 first vertex,
u16 vertex count, u8 area, u8 tile-stitched edge count, u32 flags B, then i32 neighbour per edge (-1 none).
Format cross-checked against DumbadsSkate3ModdingTools by Ethanw05 (NavPower v23 constants; no code used).
"""
from __future__ import annotations

import math
import struct
from collections import defaultdict

MAGIC = b'LWNAVMSH'
VERSION = 1
FLT_MAX_BITS = 0x7F7FFFFF
STITCH_PROBE = 0.5
STITCH_BORDER = 0.5
STITCH_HEIGHT = 1.0
GRID = 8.0


def _u(d: bytes, o: int) -> int:
    return struct.unpack_from('>I', d, o)[0]


def _v(d: bytes, o: int) -> tuple:
    return struct.unpack_from('>3f', d, o)


def find_base(d: bytes) -> int | None:
    """Offset of the nav graph's agent parameters (four small positive floats followed by min <= max bounds)."""
    for o in range(0x40, min(len(d) - 40, 0x800), 4):
        cell, radius, step, height = struct.unpack_from('>4f', d, o)
        if not (1e-3 < cell < 1 and 1e-3 < radius < 2 and 1e-3 < step < 2 and 1e-3 < height < 4):
            continue
        lo, hi = _v(d, o + 16), _v(d, o + 28)
        if all(math.isfinite(a) and math.isfinite(b) and a <= b for a, b in zip(lo, hi)):
            return o
    return None


def parse_graph(d: bytes) -> dict | None:
    """One NavPower object -> {'agent', 'bounds', 'polygons': [{'verts', 'neighbours' (local indices or -1),
    'area', 'flags', 'centre'}]}; None for objects without a graph (the 88-byte companions)."""
    base = find_base(d)
    if base is None:
        return None
    agent = struct.unpack_from('>4f', d, base)
    bounds = (_v(d, base + 16), _v(d, base + 28))
    at = None
    for i in range(base + 40, len(d) - 24, 4):
        if _u(d, i + 16) == FLT_MAX_BITS and _u(d, i + 20) == 0:
            at = i
            break
    raw = []
    while at is not None:
        r, recs = at + 36, []
        while r + 24 <= len(d) and (_u(d, r + 16) >> 16) == 0xFFFF:
            recs.append((_u(d, r), _v(d, r + 4)))
            r += 24
        if len(recs) < 3:
            raise ValueError(f'navmesh polygon at {at:#x} has {len(recs)} edges')
        raw.append((at, _v(d, at), _u(d, at + 28), recs))
        nxt = None
        for gap in range(0, 32, 4):
            q = r + gap
            if q + 24 <= len(d) and _u(d, q + 16) == FLT_MAX_BITS and _u(d, q + 20) == 0:
                nxt = q
                break
        at = nxt
    index = {p[0]: k for k, p in enumerate(raw)}
    polygons = []
    for at, centre, flags, recs in raw:
        neighbours = []
        for nb, _ in recs:
            if nb == 0:
                neighbours.append(-1)
            elif base + nb in index:
                neighbours.append(index[base + nb])
            else:
                raise ValueError(f'navmesh neighbour {nb:#x} of polygon {at:#x} is not a polygon')
        polygons.append({'verts': [v for _, v in recs], 'neighbours': neighbours, 'area': (flags >> 8) & 0xFF,
                         'flags': flags, 'centre': centre})
    return {'agent': agent, 'bounds': bounds, 'polygons': polygons}


def _inside(x: float, z: float, verts) -> bool:
    c = False
    n = len(verts)
    for i in range(n):
        x1, _, z1 = verts[i]
        x2, _, z2 = verts[(i + 1) % n]
        if (z1 > z) != (z2 > z) and x < (x2 - x1) * (z - z1) / (z2 - z1) + x1:
            c = not c
    return c


def merge_district(graphs: list[dict]) -> dict:
    """Tiles of one district -> one mesh with global polygon indices and cross-tile links."""
    polys, tile_of, bounds = [], [], [g['bounds'] for g in graphs]
    agent = graphs[0]['agent'] if graphs else (0.12, 0.35, 0.2, 1.6)
    for t, g in enumerate(graphs):
        offset = len(polys)
        for p in g['polygons']:
            polys.append({**p, 'neighbours': [n + offset if n >= 0 else -1 for n in p['neighbours']], 'stitched': 0})
            tile_of.append(t)
    grid = defaultdict(list)
    for k, p in enumerate(polys):
        xs = [v[0] for v in p['verts']]
        zs = [v[2] for v in p['verts']]
        for i in range(math.floor(min(xs) / GRID), math.floor(max(xs) / GRID) + 1):
            for j in range(math.floor(min(zs) / GRID), math.floor(max(zs) / GRID) + 1):
                grid[(i, j)].append(k)
    for k, p in enumerate(polys):
        cx, cz = p['centre'][0], p['centre'][2]
        n = len(p['verts'])
        for e in range(n):
            if p['neighbours'][e] >= 0:
                continue
            a, b = p['verts'][e], p['verts'][(e + 1) % n]
            mx, my, mz = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2
            lo, hi = bounds[tile_of[k]]
            if min(mx - lo[0], hi[0] - mx, mz - lo[2], hi[2] - mz) > STITCH_BORDER:
                continue
            dx, dz = b[0] - a[0], b[2] - a[2]
            length = math.hypot(dx, dz)
            if length < 1e-6:
                continue
            nx, nz = dz / length, -dx / length
            if (mx - cx) * nx + (mz - cz) * nz < 0:
                nx, nz = -nx, -nz
            px, pz = mx + nx * STITCH_PROBE, mz + nz * STITCH_PROBE
            for q in grid.get((math.floor(px / GRID), math.floor(pz / GRID)), []):
                if tile_of[q] == tile_of[k]:
                    continue
                o = polys[q]
                y = sum(v[1] for v in o['verts']) / len(o['verts'])
                if abs(y - my) <= STITCH_HEIGHT and _inside(px, pz, o['verts']):
                    p['neighbours'][e] = q
                    p['stitched'] += 1
                    break
    return {'agent': agent, 'polygons': polys}


def write_navmesh(districts: dict[str, dict]) -> bytes:
    out = bytearray(MAGIC)
    out += struct.pack('<II', VERSION, len(districts))
    for name, mesh in sorted(districts.items()):
        raw = name.encode('ascii')
        out += struct.pack('<B', len(raw)) + raw
        out += struct.pack('<4f', *mesh['agent'])
        verts = [v for p in mesh['polygons'] for v in p['verts']]
        out += struct.pack('<I', len(verts))
        for v in verts:
            out += struct.pack('<3f', *v)
        out += struct.pack('<I', len(mesh['polygons']))
        first = 0
        for p in mesh['polygons']:
            n = len(p['verts'])
            out += struct.pack('<IHBBI', first, n, p['area'], min(p.get('stitched', 0), 255), p['flags'])
            out += struct.pack(f'<{n}i', *p['neighbours'])
            first += n
    return bytes(out)


def read_navmesh(data: bytes) -> dict[str, dict]:
    if data[:8] != MAGIC:
        raise ValueError('not a navmesh.bin')
    version, count = struct.unpack_from('<II', data, 8)
    if version != VERSION:
        raise ValueError(f'navmesh.bin version {version}')
    at, out = 16, {}
    for _ in range(count):
        n = data[at]
        name = data[at + 1:at + 1 + n].decode('ascii')
        at += 1 + n
        agent = struct.unpack_from('<4f', data, at)
        at += 16
        nv = struct.unpack_from('<I', data, at)[0]
        at += 4
        verts = [struct.unpack_from('<3f', data, at + 12 * i) for i in range(nv)]
        at += 12 * nv
        npoly = struct.unpack_from('<I', data, at)[0]
        at += 4
        polys = []
        for _ in range(npoly):
            first, vc, area, stitched, flags = struct.unpack_from('<IHBBI', data, at)
            at += 12
            neighbours = list(struct.unpack_from(f'<{vc}i', data, at))
            at += 4 * vc
            polys.append({'verts': verts[first:first + vc], 'neighbours': neighbours, 'area': area,
                          'stitched': stitched, 'flags': flags})
        out[name] = {'agent': agent, 'polygons': polys}
    return out


def summary(mesh: dict) -> dict:
    areas = defaultdict(int)
    for p in mesh['polygons']:
        areas[f"{p['area']:#04x}"] += 1
    return {'polygons': len(mesh['polygons']), 'areas': dict(sorted(areas.items())),
            'stitched_edges': sum(p.get('stitched', 0) for p in mesh['polygons']),
            'boundary_edges': sum(1 for p in mesh['polygons'] for n in p['neighbours'] if n < 0),
            'agent': [round(a, 4) for a in mesh['agent']]}
