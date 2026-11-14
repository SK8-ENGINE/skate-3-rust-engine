"""Software render of a living-world GLB (ped or vehicle) to PNG, for visual checks without the engine.

usage: py -3.13 render_glb.py <model.glb> <out.png> [--view side|front|rear|top|three_quarter] [--skip-role windows]
       [--clip-x 0.0] [--size 640] [--untextured] [--pose dump.json --pose-index 0 [--bone-basis auto|none|skater]]
       [--tint 'r,g,b;r,g,b;gain']

Draws LOD0 in the bind pose (glTF positions are model space), textured with the base-colour map
(nearest sample) and simple lambert shading; ``--skip-role windows`` leaves out primitives whose
``extras.role`` matches (to look into a car), ``--clip-x`` drops triangles with all x above the value
(a cut through the cabin). Reference tool only; nothing here ships.
"""
import argparse
import io
import json
import struct

import numpy as np
from pathlib import Path
from PIL import Image

COMPONENTS = {5126: np.float32, 5123: np.uint16, 5125: np.uint32, 5121: np.uint8}
WIDTH = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}


def load(path):
    data = open(path, 'rb').read()
    length = struct.unpack_from('<I', data, 12)[0]
    doc = json.loads(data[20:20 + length])
    at = 20 + length
    bin_length = struct.unpack_from('<I', data, at)[0]
    blob = data[at + 8:at + 8 + bin_length]

    def accessor(i):
        a = doc['accessors'][i]
        view = doc['bufferViews'][a['bufferView']]
        dtype = COMPONENTS[a['componentType']]
        n = WIDTH[a['type']]
        start = view.get('byteOffset', 0) + a.get('byteOffset', 0)
        return np.frombuffer(blob, dtype=dtype, count=a['count'] * n, offset=start).reshape(a['count'], n)

    def image(i):
        view = doc['bufferViews'][doc['images'][i]['bufferView']]
        start = view.get('byteOffset', 0)
        return np.asarray(Image.open(io.BytesIO(blob[start:start + view['byteLength']])).convert('RGBA'))
    return doc, accessor, image


def rotation_angle(a, b):
    ra = a[:3, :3] / np.linalg.norm(a[:3, :3], axis=0)
    rb = b[:3, :3] / np.linalg.norm(b[:3, :3], axis=0)
    return np.arccos(np.clip((np.trace(ra.T @ rb) - 1) / 2, -1, 1))


def skin_matrices(doc, accessor, pose_file, index, bone_basis='auto'):
    """Joint skinning matrices (world x inverse bind) from a ped pose dump (``peds_tests``
    ``living_world_ped_pose_dump``: native model-space matrices of the 50-bone rig, column-major).
    Joint world = bone global x bone basis; bones without clip data follow their rig parent with
    the GLB's bind offset (same rule as the engine's ``follower_offsets``). The bone basis follows
    the engine's ``ped_bone_basis`` (fix10): identity for the shipped ped GLBs (their bind frames
    are the rig's reference frames), the skater ``render_basis`` only for a GLB written that way;
    ``bone_basis='skater'`` reproduces the pre-fix10 (warped) skin."""
    dump = json.load(open(pose_file))
    names = [n.upper() for n in dump['names']]
    parents = dump['parents']
    animated = dump['animated']
    globals_ = [np.array(m, dtype=np.float64).reshape(4, 4).T for m in dump['poses'][index]['bones']]
    skater = np.array([[1, 0, 0, 0], [0, 0, 1, 0], [0, -1, 0, 0], [0, 0, 0, 1]], dtype=np.float64)
    skin = doc['skins'][0]
    joints = [doc['nodes'][j]['name'].upper() for j in skin['joints']]
    ibm = accessor(skin['inverseBindMatrices']).astype(np.float64).reshape(-1, 4, 4).transpose(0, 2, 1)
    glb_bind = {names.index(n): np.linalg.inv(ibm[k]) for k, n in enumerate(joints) if n in names}
    if bone_basis == 'auto':
        ref = [p for p in dump['poses'] if p['label'] == 'reference']
        basis = np.eye(4)
        if ref:
            rg = [np.array(m, dtype=np.float64).reshape(4, 4).T for m in ref[0]['bones']]
            scores = [(sum(rotation_angle(rg[i] @ b, m) for i, m in glb_bind.items()), k) for k, b in enumerate((np.eye(4), skater))]
            basis = (np.eye(4), skater)[min(scores)[1]]
    else:
        basis = skater if bone_basis == 'skater' else np.eye(4)
    bind = {i: m @ np.linalg.inv(basis) for i, m in glb_bind.items()}
    world = {}
    for i in range(len(names)):
        if animated[i]:
            world[i] = globals_[i] @ basis
        elif parents[i] >= 0 and parents[i] in world and i in bind and parents[i] in bind:
            world[i] = world[parents[i]] @ np.linalg.inv(bind[parents[i]]) @ bind[i]
    world = {i: m @ basis for i, m in world.items()}
    out = []
    for k, n in enumerate(joints):
        i = names.index(n)
        out.append(world[i] @ ibm[k] if i in world else np.eye(4))
    return np.array(out)


def tint(tex, chassis, secondary, gain=1.0):
    """The engine's chassis tint (``skate-game`` ``living_world::vehicles::tint_rgba8``): retail's
    vehicle_defaultPS rule, the same as the ped shaders (secondary -> i_colorize_red, chassis ->
    i_colorize_blue), both colours scaled by ``gain`` (1 = retail)."""
    return ped_colorize(tex, [c * gain for c in secondary[:3]], [c * gain for c in chassis[:3]])


def ped_colorize(tex, red, blue):
    """Retail's ped shader rule (skate-core ``living_world::peds::colorize``, decoded from
    livingworld_stamp_defaultPS): where G^2 < 0.001225, rgb = sqrt(R^2 x red + B^2 x blue)."""
    px = tex[..., :3].astype(float) / 255.0
    lin = px * px
    mask = lin[..., 1] < 0.001225
    out = px.copy()
    for i in range(3):
        out[..., i] = np.where(mask, np.sqrt(np.clip(lin[..., 0] * red[i] + lin[..., 2] * blue[i], 0, None)), px[..., i])
    res = tex.copy()
    res[..., :3] = np.clip(np.round(out * 255), 0, 255).astype(np.uint8)
    return res


def ped_colorized(material):
    """Engine rule (skate-game ``ped_material_colorized``): the material type from the export, else
    the body slot for older exports."""
    extras = material.get('extras')
    if extras is not None:
        return extras.get('shader') in ('pedestrian_high_stamp', 'pedestrian_low')
    return material.get('name', '').startswith('Rostral_')


def ped_look_tints(glb, model, index):
    """(red, blue) tint of a livingworld_models record for a palette index (one rand r: secondary[r % n],
    chassis[r % m]) from the tables.json next to the models folder."""
    import json
    tables = json.loads((Path(glb).resolve().parent.parent/'tables.json').read_text(encoding='utf-8'))
    f = tables['classes']['livingworld_models'][model]['fields']
    pick = lambda v: [v[index % len(v)][k] for k in 'xyz'] if v else [1.0, 1.0, 1.0]
    return pick(f.get('secondary_colours')), pick(f.get('chassis_colours'))


def render(path, out, view='side', skip=(), clip_x=None, size=640, textured=True, pose=None, pose_index=0, tints=None, bone_basis='auto', ped_tints=None):
    doc, accessor, image = load(path)
    mesh = doc['meshes'][0]
    skin_m = skin_matrices(doc, accessor, pose, pose_index, bone_basis) if pose else None
    tris = []
    for prim in mesh['primitives']:
        if prim.get('extras', {}).get('role') in skip:
            continue
        pos = accessor(prim['attributes']['POSITION']).astype(np.float64)
        if skin_m is not None:
            j = accessor(prim['attributes']['JOINTS_0']).astype(int)
            w = accessor(prim['attributes']['WEIGHTS_0']).astype(np.float64)
            if w.max() > 1.5:
                w = w / 255.0
            h = np.concatenate([pos, np.ones((len(pos), 1))], 1)
            pos = sum(w[:, k:k + 1] * np.einsum('nij,nj->ni', skin_m[j[:, k]], h) for k in range(4))[:, :3]
        uv = accessor(prim['attributes']['TEXCOORD_0']).astype(np.float64)
        idx = accessor(prim['indices']).reshape(-1, 3)
        material = doc['materials'][prim['material']]
        tex = None
        if textured and 'baseColorTexture' in material.get('pbrMetallicRoughness', {}):
            tex = image(doc['textures'][material['pbrMetallicRoughness']['baseColorTexture']['index']]['source'])
            if tints and material.get('extras', {}).get('kind') == 'vehicle_chassis':
                tex = tint(tex, *tints)
            if ped_tints and ped_colorized(material):
                tex = ped_colorize(tex, *ped_tints)
        tris.append((pos, uv, idx, tex))
    rot = {'side': np.array([[0, 0, 1], [0, 1, 0], [-1, 0, 0]]),       # looking at the car's +x side, +z to the right
           'front': np.eye(3), 'rear': np.array([[-1, 0, 0], [0, 1, 0], [0, 0, -1]]),
           'top': np.array([[0, 0, 1], [1, 0, 0], [0, 1, 0]]),
           'three_quarter': None}[view]
    if rot is None:
        a, b = np.radians(35), np.radians(20)
        ry = np.array([[np.cos(a), 0, np.sin(a)], [0, 1, 0], [-np.sin(a), 0, np.cos(a)]])
        rx = np.array([[1, 0, 0], [0, np.cos(b), -np.sin(b)], [0, np.sin(b), np.cos(b)]])
        rot = rx @ ry
    allpos = np.concatenate([p for p, _, _, _ in tris]) @ rot.T
    lo, hi = allpos.min(0), allpos.max(0)
    scale = 0.9 * size / max(hi[0] - lo[0], hi[1] - lo[1])
    centre = (lo + hi) / 2
    colour = np.full((size, size, 3), 235, np.uint8)
    depth = np.full((size, size), -np.inf)
    light = np.array([0.4, 0.8, 0.45])
    light /= np.linalg.norm(light)
    for pos, uv, idx, tex in tris:
        p = pos @ rot.T
        sx = (p[:, 0] - centre[0]) * scale + size / 2
        sy = size / 2 - (p[:, 1] - centre[1]) * scale
        for a, b, c in idx:
            if clip_x is not None and min(pos[a, 0], pos[b, 0], pos[c, 0]) > clip_x:
                continue
            xs, ys = np.array([sx[a], sx[b], sx[c]]), np.array([sy[a], sy[b], sy[c]])
            x0, x1 = max(int(xs.min()), 0), min(int(xs.max()) + 1, size)
            y0, y1 = max(int(ys.min()), 0), min(int(ys.max()) + 1, size)
            if x0 >= x1 or y0 >= y1:
                continue
            gx, gy = np.meshgrid(np.arange(x0, x1) + 0.5, np.arange(y0, y1) + 0.5)
            d = (ys[1] - ys[2]) * (xs[0] - xs[2]) + (xs[2] - xs[1]) * (ys[0] - ys[2])
            if abs(d) < 1e-12:
                continue
            w0 = ((ys[1] - ys[2]) * (gx - xs[2]) + (xs[2] - xs[1]) * (gy - ys[2])) / d
            w1 = ((ys[2] - ys[0]) * (gx - xs[2]) + (xs[0] - xs[2]) * (gy - ys[2])) / d
            w2 = 1 - w0 - w1
            inside = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
            if not inside.any():
                continue
            z = w0 * p[a, 2] + w1 * p[b, 2] + w2 * p[c, 2]
            region = depth[y0:y1, x0:x1]
            hit = inside & (z > region)
            if not hit.any():
                continue
            normal = np.cross(p[b] - p[a], p[c] - p[a])
            shade = 0.35 + 0.65 * abs(normal @ (rot @ light)) / (np.linalg.norm(normal) + 1e-20)
            if tex is not None:
                u = w0 * uv[a, 0] + w1 * uv[b, 0] + w2 * uv[c, 0]
                v = w0 * uv[a, 1] + w1 * uv[b, 1] + w2 * uv[c, 1]
                h, w = tex.shape[:2]
                ti = np.clip((v % 1.0) * h, 0, h - 1).astype(int)
                tj = np.clip((u % 1.0) * w, 0, w - 1).astype(int)
                rgb = tex[ti, tj, :3].astype(float)
            else:
                rgb = np.full(gx.shape + (3,), 190.0)
            block = colour[y0:y1, x0:x1]
            block[hit] = np.clip(rgb[hit] * shade, 0, 255).astype(np.uint8)
            region[hit] = z[hit]
    Image.fromarray(colour).save(out)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('glb')
    parser.add_argument('out')
    parser.add_argument('--view', default='side')
    parser.add_argument('--skip-role', action='append', default=[])
    parser.add_argument('--clip-x', type=float)
    parser.add_argument('--size', type=int, default=640)
    parser.add_argument('--untextured', action='store_true')
    parser.add_argument('--pose', help='ped pose dump (peds_tests living_world_ped_pose_dump)')
    parser.add_argument('--pose-index', type=int, default=0)
    parser.add_argument('--bone-basis', default='auto', choices=['auto', 'none', 'skater'],
                        help="ped joint frame basis (auto = engine rule; skater = pre-fix10 warped skin)")
    parser.add_argument('--tint', help='chassis r,g,b[;secondary r,g,b[;gain]] (retail vehicle_defaultPS tint rule)')
    parser.add_argument('--ped-tint', help="ped tint pair 'r,g,b;r,g,b' (red mask; blue mask), retail ped shader rule")
    parser.add_argument('--ped-look', help='MODEL:INDEX: the tint pair of a livingworld_models record (tables.json)')
    a = parser.parse_args()
    ped_tints = None
    if a.ped_tint:
        ped_tints = tuple([float(x) for x in part.split(',')] for part in a.ped_tint.split(';'))
    if a.ped_look:
        model, index = a.ped_look.rsplit(':', 1)
        ped_tints = ped_look_tints(a.glb, model, int(index))
    tints = None
    if a.tint:
        parts = a.tint.split(';')
        tints = ([float(x) for x in parts[0].split(',')], [float(x) for x in parts[1].split(',')] if len(parts) > 1 else [1.0, 0.0, 0.0],
                 float(parts[2]) if len(parts) > 2 else 1.0)
    render(a.glb, a.out, a.view, tuple(a.skip_role), a.clip_x, a.size, not a.untextured, a.pose, a.pose_index, tints, a.bone_basis, ped_tints)
