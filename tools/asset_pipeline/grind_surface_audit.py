"""Audit every DMO template's and car model's grind rails against the collision the engine uses for it.

For each rail segment, sampled every 2 cm in model space:
- supported: a collision triangle lies directly under the sample, its surface within 1 cm of the rail height
  (the board grinds on the shape, not in the air);
- obstructed: a collision triangle rises from 2 to 15 cm above the rail within 0.1 m of the rail line across it
  (a face the board meets head-on while grinding: the 2026-10-15 bench seam bails).

Inputs: an installation's `private/native-props/<map>.json` (`grind_splines`, `collision` per template id) and
`private/living_world/vehicles.json` (`models.<key>.grind_splines` / `collision`). Templates or models without a
retail collision shape are listed as unchecked (they collide with their render mesh).

    python -I tools/asset_pipeline/grind_surface_audit.py <installation assets dir> [--json out.json]
"""
import argparse
import json
import struct
from pathlib import Path

import numpy as np

STEP = 0.02
SUPPORT = 0.01
BOARD_HALF_WIDTH = 0.10
CLEAR_LOW, CLEAR_HIGH = 0.02, 0.15


def segments(rails):
    """(start, end) model-space points of every rail segment (start at payload byte 48, end at byte 80)."""
    out = []
    for rail in rails:
        for payload in rail.get('native_segment_payloads', []):
            raw = bytes.fromhex(payload)
            if len(raw) < 92:
                continue
            a = np.array(struct.unpack_from('>3f', raw, 48))
            b = np.array(struct.unpack_from('>3f', raw, 80))
            if np.all(np.isfinite(a)) and np.all(np.isfinite(b)) and np.linalg.norm(b - a) > 1e-4:
                out.append((a, b))
    return out


def down_hits(tris, p):
    """Heights of the triangles under point p (vertical ray, any winding)."""
    a, b, c = tris[:, 0], tris[:, 1], tris[:, 2]
    v0, v1 = b - a, c - a
    d = np.array([0.0, -1.0, 0.0])
    h = np.cross(d, v1)
    det = np.einsum('ij,ij->i', v0, h)
    ok = np.abs(det) > 1e-9
    inv = np.where(ok, 1.0 / np.where(ok, det, 1.0), 0.0)
    s = p - a
    u = np.einsum('ij,ij->i', s, h) * inv
    q = np.cross(s, v0)
    v = (q @ d) * inv
    t = np.einsum('ij,ij->i', v1, q) * inv
    hit = ok & (u >= -1e-6) & (v >= -1e-6) & (u + v <= 1 + 1e-6)
    return p[1] - t[hit]


def audit(rails, shape):
    tris = np.array(shape['triangles'], dtype=np.float64)
    bad_support = bad_clear = total = 0
    worst = []
    for a, b in segments(rails):
        length = np.linalg.norm(b - a)
        direction = (b - a) / length
        across = np.cross(direction, [0.0, 1.0, 0.0])
        across = across / (np.linalg.norm(across) or 1.0)
        for k in range(max(1, int(length / STEP)) + 1):
            p = a + direction * min(k * STEP, length)
            total += 1
            heights = down_hits(tris, p + np.array([0.0, 0.5, 0.0]))
            if heights.size == 0 or np.min(np.abs(heights - p[1])) > SUPPORT:
                bad_support += 1
            blocked = False
            for side in (-BOARD_HALF_WIDTH, 0.0, BOARD_HALF_WIDTH):
                q = p + across * side + np.array([0.0, 1.0, 0.0])
                h = down_hits(tris, q)
                if h.size and np.any((h > p[1] + CLEAR_LOW) & (h < p[1] + CLEAR_HIGH)):
                    blocked = True
            if blocked:
                bad_clear += 1
                if len(worst) < 3:
                    worst.append([round(float(x), 3) for x in p])
    return dict(samples=total, unsupported=bad_support, obstructed=bad_clear, first_obstructed=worst)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('assets', type=Path)
    parser.add_argument('--json', type=Path)
    args = parser.parse_args()
    report = {}
    for sidecar in sorted((args.assets / 'private/native-props').glob('*.json')):
        doc = json.loads(sidecar.read_text())
        for key, rails in doc.get('grind_splines', {}).items():
            shape = doc.get('collision', {}).get(key)
            report[f'prop {key}'] = audit(rails, shape) if shape else 'unchecked (render mesh)'
    vehicles = args.assets / 'private/living_world/vehicles.json'
    if vehicles.exists():
        for key, model in json.loads(vehicles.read_text()).get('models', {}).items():
            if model.get('grind_splines'):
                report[f'car {key}'] = audit(model['grind_splines'], model['collision']) if model.get('collision') else 'unchecked (box)'
    for name, row in report.items():
        if isinstance(row, dict):
            flag = '' if row['obstructed'] == 0 and row['unsupported'] <= row['samples'] * 0.05 else '  <-- check'
            print(f"{name:28s} samples {row['samples']:5d} unsupported {row['unsupported']:5d} obstructed {row['obstructed']:5d}{flag} {row['first_obstructed'] or ''}")
        else:
            print(f'{name:28s} {row}')
    if args.json:
        args.json.write_text(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
