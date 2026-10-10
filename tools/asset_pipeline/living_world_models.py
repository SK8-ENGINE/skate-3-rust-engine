"""Living world ped / hand-prop models: binary recipe (``living_world.parse_recipe``) to glTF binary.

One ``living_world/models/<recipe>.glb`` per recipe, written straight from the retail rx2 arenas of
``livingworld.big`` (no retargeting): the skeleton is the recipe's main part's own bind skeleton
(peds: 39 named bones, ``Hips`` root; bones the arena stores without a parent hang off the rig root
at their bind pose), every part (``Rostral`` body, ``Hair``, ``Accessory``, ``Equipment``) is one
primitive, and the two LODs are two meshes on two nodes named ``LOD0`` / ``LOD1`` sharing that skin.
Space: the arena's own (metres, Y up, the ped standing on y = 0), row-vector matrices written as
glTF column-major like ``character_glb``. Matching these bones to the 50-bone animation rig of
``PedestrianSkeletonPres.abin`` is the animation player's job (milestone M2), by bone name.
Materials: diffuse (+ alpha as its alpha channel, MASK) and the normal map converted like the
marquee pipeline (``native_roster``). Unskinned props (hand props, ``zprop_*``) get no skin.
"""
from __future__ import annotations

import io
import struct
from pathlib import Path

import numpy as np
from PIL import Image

from tools.asset_pipeline.character_glb import Glb


def _normal_png(source: Path) -> bytes:
    """Retail two-channel normal map (x in alpha, y in green) to a tangent-space RGB normal map."""
    a = np.asarray(Image.open(source).convert('RGBA'), dtype=float)
    x = a[:, :, 3] / 127.5 - 1.
    y = a[:, :, 1] / 127.5 - 1.
    s = np.maximum(np.sqrt(x * x + y * y), 1.)
    x /= s
    y /= s
    z = np.sqrt(np.maximum(0., 1. - x * x - y * y))
    buffer = io.BytesIO()
    Image.fromarray(np.rint((np.stack((x, y, z), 2) * .5 + .5) * 255).astype('uint8')).save(buffer, format='PNG')
    return buffer.getvalue()


def _bind_worlds(bones: list[dict]) -> dict[str, np.ndarray]:
    return {b['name']: np.asarray(b['bind_matrix'], dtype=np.float64) for b in bones}


def write_glb(recipe: dict, arena_path, texture_path, output: Path, rx2) -> dict:
    """Write one recipe. ``arena_path(lod) -> Path`` gives the extracted rx2 of a LOD entry,
    ``texture_path(texture id) -> Path`` a decoded PNG. Returns a summary (bones, vertices, triangles)."""
    glb = Glb()
    parsed = {}
    for part in recipe['parts']:
        for level, lod in enumerate(part['lods'][:2]):
            parsed[(part['slot'], level)] = rx2.parse_rx2(str(arena_path(lod)))
    # The skeleton: the body part's LOD0 bones (the part with the most bones), others matched by name.
    main = max(parsed.values(), key=lambda d: len(d['bones']))
    bones = main['bones']
    skinned = bool(bones)
    names = [b['name'] for b in bones]
    index = {n: i for i, n in enumerate(names)}
    worlds = _bind_worlds(bones)
    for data in parsed.values():  # bones a secondary part has and the body lacks
        for bone in data['bones']:
            if bone['name'] not in index:
                index[bone['name']] = len(names)
                names.append(bone['name'])
                bones = bones + [bone]
                worlds[bone['name']] = np.asarray(bone['bind_matrix'], dtype=np.float64)
    nodes, root_children = [], []
    if skinned:
        parents = []
        for bone in bones:
            parent = bone.get('parent_name') if bone.get('parent', -1) >= 0 else None
            parents.append(index.get(parent) if parent in index else None)
        for i, bone in enumerate(bones):
            world = worlds[bone['name']]
            parent = parents[i]
            local = world @ np.linalg.inv(worlds[names[parent]]) if parent is not None else world
            nodes.append({'name': bone['name'], 'matrix': local.ravel().tolist()})
        for i, parent in enumerate(parents):
            if parent is None:
                root_children.append(i)
            else:
                nodes[parent].setdefault('children', []).append(i)
        inverse = glb.accessor([np.linalg.inv(worlds[n]).ravel() for n in names], 'MAT4')
    materials, meshes, stats = {}, [], {'bones': len(names), 'lods': []}
    for level in (0, 1):
        primitives, vertices, triangles = [], 0, 0
        for part in recipe['parts']:
            if len(part['lods']) <= level:
                continue
            lod = part['lods'][level]
            data = parsed[(part['slot'], level)]
            raw = [m for m in data['meshes'] if m.get('positions') and m.get('indices')]
            if not raw and lod['material'] is None:
                continue  # an empty LOD (no mesh, no material: zprop_saftybarrier LOD1)
            if len(raw) != 1:
                raise ValueError(f"{recipe['name']}/{part['slot']} LOD{level}: {len(raw)} meshes")
            mesh = raw[0]
            positions = np.asarray(mesh['positions'], dtype=np.float64)
            normals = np.asarray(mesh['normals'], dtype=np.float64) if mesh.get('normals') else None
            if normals is None or len(normals) != len(positions):
                normals = np.zeros_like(positions)
                tris = np.asarray(mesh['indices']).reshape(-1, 3)
                for a, b, c in tris:
                    n = np.cross(positions[b] - positions[a], positions[c] - positions[a])
                    normals[[a, b, c]] += n
            normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-20)
            attributes = {'POSITION': glb.accessor(positions, 'VEC3', bounds=True),
                          'NORMAL': glb.accessor(normals, 'VEC3'),
                          'TEXCOORD_0': glb.accessor(mesh['uvs'], 'VEC2')}
            if skinned and mesh.get('skin_bones'):
                part_names = [b['name'] for b in data['bones']]
                joints = np.zeros((len(positions), 4), dtype=np.uint16)
                weights = np.asarray(mesh['skin_weights'], dtype=np.float32)
                for v, influence in enumerate(mesh['skin_bones']):
                    joints[v] = [index[part_names[b]] for b in influence]
                total = weights.sum(axis=1, keepdims=True)
                if (total <= 0).any():
                    raise ValueError(f"{recipe['name']}/{part['slot']}: vertex without skin weight")
                attributes['JOINTS_0'] = glb.accessor(joints, 'VEC4', 5123)
                attributes['WEIGHTS_0'] = glb.accessor(weights / total, 'VEC4')
            key = (lod['material'], tuple(sorted(lod['textures'].items())))
            if key not in materials:
                textures = lod['textures']
                if 'diffuse' not in textures:
                    raise ValueError(f"{recipe['name']}/{part['slot']}: no diffuse texture")
                diffuse = Image.open(texture_path(textures['diffuse'])).convert('RGBA')
                if 'alpha' in textures:
                    alpha = Image.open(texture_path(textures['alpha'])).convert('RGB').getchannel('R')
                    if alpha.size != diffuse.size:  # some peds ship a smaller alpha map than the diffuse
                        alpha = alpha.resize(diffuse.size, Image.BILINEAR)
                    diffuse.putalpha(alpha)
                buffer = io.BytesIO()
                diffuse.save(buffer, format='PNG')
                material = {'name': f"{part['slot']}_{lod['material']}",
                            'pbrMetallicRoughness': {'baseColorTexture': glb.texture(buffer.getvalue()),
                                                     'metallicFactor': 0.0, 'roughnessFactor': 0.72},
                            'alphaMode': 'MASK' if 'alpha' in textures else 'OPAQUE'}
                if 'alpha' in textures:
                    material['alphaCutoff'] = 0.5
                shader = recipe.get('shaders', {}).get(lod['material'] or '')
                if shader:  # the retail material type: the game recolours pedestrian_* mask texels
                    material['extras'] = {'shader': shader}
                if 'normal' in textures:
                    material['normalTexture'] = glb.texture(_normal_png(texture_path(textures['normal'])))
                glb.doc['materials'].append(material)
                materials[key] = len(glb.doc['materials']) - 1
            primitives.append({'attributes': attributes, 'material': materials[key],
                               'indices': glb.accessor(np.asarray(mesh['indices'], dtype=np.uint32), 'SCALAR', 5125),
                               'extras': {'slot': part['slot']}})
            vertices += len(positions)
            triangles += len(mesh['indices']) // 3
        if not primitives:
            continue
        meshes.append({'name': f'LOD{level}', 'primitives': primitives})
        stats['lods'].append({'vertices': vertices, 'triangles': triangles})
    lod_nodes = []
    for level, mesh in enumerate(meshes):
        node = {'name': f'LOD{level}', 'mesh': level}
        if skinned:
            node['skin'] = 0
        lod_nodes.append(len(nodes))
        nodes.append(node)
    root = len(nodes)
    nodes.append({'name': recipe['name'], 'children': root_children + lod_nodes})
    glb.doc.update(nodes=nodes, meshes=meshes, scenes=[{'nodes': [root]}], scene=0,
                   extras={'recipe': recipe['name'], 'lods': len(meshes)})
    if skinned:
        glb.doc['skins'] = [{'joints': list(range(len(names))), 'inverseBindMatrices': inverse}]
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix('.tmp')
    glb.save(temporary)
    temporary.replace(output)
    return stats


def read_glb(path: Path) -> dict:
    """The JSON chunk of a glb (tests and validation)."""
    data = path.read_bytes()
    magic, version, length = struct.unpack_from('<III', data, 0)
    if magic != 0x46546C67 or version != 2:
        raise ValueError(f'{path.name} is not glTF 2.0 binary')
    chunk = struct.unpack_from('<I', data, 12)[0]
    import json
    return json.loads(data[20:20 + chunk])


def export_models(game_root: Path, manifest: dict, output: Path, work: Path, report=print) -> dict:
    """Convert every recipe of ``manifest`` (``living_world.model_manifest``). Returns
    {recipe: {status, file?, stats?, error?}}; a failed recipe does not stop the others."""
    from tools.extract_default_skater import decode_texture, import_rx2_parser
    from tools.asset_pipeline.retail_character import RX2
    from tools.owned_game.big import BigArchive
    big = BigArchive(game_root/manifest['archive'])
    entries = {e.path.lower(): e for e in big.entries}
    parser = import_rx2_parser(Path(__file__).resolve().parents[1]/'vendor/utt')
    source, decoded = work/'source', work/'decoded'

    def extract(path: str) -> Path:
        dest = source/path
        if not dest.exists():
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(big.read(entries[path.lower()]))
        return dest

    def texture(tid: str) -> Path:
        dest = decoded/f'{tid}.png'
        if not dest.exists():
            decode_texture(parser, extract(f'data/content/livingworld/texture/0x{tid}.rx2'), dest)
        return dest

    results = {}
    for name, recipe in sorted(manifest['recipes'].items()):
        try:
            stats = write_glb(recipe, lambda lod: extract(lod['arena']), texture, output/f'{name}.glb', RX2)
            results[name] = {'status': 'ready', 'file': f'models/{name}.glb', 'kind': recipe['kind'], **stats}
        except (ValueError, KeyError, RuntimeError, IndexError, struct.error, OSError) as error:
            if isinstance(error, OSError) and not isinstance(error, FileNotFoundError):
                raise  # disk full / permissions abort setup
            results[name] = {'status': 'unavailable', 'kind': recipe['kind'], 'error': str(error)}
            report(f'Living world model {name} unavailable: {error}')
    return results
