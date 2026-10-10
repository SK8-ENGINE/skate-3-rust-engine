"""Ped hand props (``livingworld_handprops``) as glTF binary, from the retail DMO templates (b87, b89).

Retail (``82E3DE18``): the hand prop record's ``model`` names a ``dmo_models`` record; its first field
(``Hash_73061AB91FDAE615``, a u64) is a DMO template id the dynamic-objects manager creates the held object from.
The ``_lw`` templates live in ``worlddmo.big`` ``DMO_Global`` (one mesh each), which ``dynamic_props.prepare_catalog``
already reads; placed props only export the templates a map places, so the hand props get their own files here.

Output: ``living_world/hand_props/<handprop key>.glb`` (the template's model space: ``model_matrix @ matrix`` baked
in, metres, Y up, row vectors written column-major) and ``living_world/hand_props.json``
``{"version", "props": {key: {"model", "template", "file", "characteristics"} | {"model", "error"}}}``. Records whose
model resolves to no template are listed with an error (stock: ``orange``), never guessed.
"""
from __future__ import annotations

import io
import json
from pathlib import Path

import numpy as np
from PIL import Image

from tools.asset_pipeline.character_glb import Glb

VERSION = 1
MODEL_ID_FIELD = 'Hash_73061AB91FDAE615'


def model_ids(tables: dict) -> dict[str, dict]:
    """Hand prop key -> {model, template} from tables.json (``livingworld_handprops`` -> ``dmo_models``)."""
    classes = tables['classes']
    models = classes.get('dmo_models', {})
    out = {}
    for key, record in sorted(classes.get('livingworld_handprops', {}).items()):
        model = (record['fields'].get('model') or {}).get('key')
        if not model:
            continue
        raw = (models.get(model) or {}).get('fields', {}).get(MODEL_ID_FIELD)
        raw = raw.get('hex') if isinstance(raw, dict) else raw  # tables.json keeps a UInt64 as {"type", "hex"}
        out[key] = {'model': model, 'template': f'{int(raw, 16):016X}' if isinstance(raw, str) and raw else None}
    return out


def write_glb(template: dict, textures: dict, output: Path) -> dict:
    """One template's meshes as one glb node (no skin)."""
    glb = Glb()
    base = np.asarray(template['model_matrix'], dtype=np.float64) @ np.asarray(template['matrix'], dtype=np.float64)
    normal_matrix = np.linalg.inv(base[:3, :3]).T
    primitives, materials, vertices, triangles = [], {}, 0, 0
    with np.load(template['npz'], allow_pickle=False) as arrays:
        for mesh in template['meshes']:
            i = mesh['index']
            positions = np.asarray(arrays[f'vertices_{i}'], dtype=np.float64) @ base[:3, :3] + base[3, :3]
            faces = np.asarray(arrays[f'faces_{i}'], dtype=np.uint32).reshape(-1, 3)
            if np.linalg.det(base[:3, :3]) < 0:
                faces = faces[:, [0, 2, 1]]
            if f'normals_{i}' in arrays.files:
                normals = np.asarray(arrays[f'normals_{i}'], dtype=np.float64) @ normal_matrix
            else:
                normals = np.zeros_like(positions)
                for a, b, c in faces:
                    normals[[a, b, c]] += np.cross(positions[b] - positions[a], positions[c] - positions[a])
            normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-20)
            attributes = {'POSITION': glb.accessor(positions, 'VEC3', bounds=True), 'NORMAL': glb.accessor(normals, 'VEC3')}
            if f'uvs_{i}' in arrays.files:
                attributes['TEXCOORD_0'] = glb.accessor(np.asarray(arrays[f'uvs_{i}'], dtype=np.float32), 'VEC2')
            texture_id = mesh.get('texture_id')
            if texture_id not in materials:
                material = {'name': f'handprop_{texture_id}', 'pbrMetallicRoughness': {'metallicFactor': 0.0, 'roughnessFactor': 0.72}}
                entry = textures.get(texture_id) if texture_id else None
                if entry is not None and 'TEXCOORD_0' in attributes:
                    pixels = np.frombuffer(Path(entry['rgba']).read_bytes(), dtype=np.uint8).reshape(entry['height'], entry['width'], 4)
                    buffer = io.BytesIO()
                    Image.fromarray(pixels, 'RGBA').save(buffer, format='PNG')
                    material['pbrMetallicRoughness']['baseColorTexture'] = glb.texture(buffer.getvalue())
                glb.doc['materials'].append(material)
                materials[texture_id] = len(glb.doc['materials']) - 1
            primitives.append({'attributes': attributes, 'material': materials[texture_id],
                               'indices': glb.accessor(faces.ravel(), 'SCALAR', 5125)})
            vertices += len(positions)
            triangles += len(faces)
    if not primitives:
        raise ValueError('template has no mesh')
    glb.doc.update(nodes=[{'name': 'LOD0', 'mesh': 0}], meshes=[{'name': 'LOD0', 'primitives': primitives}],
                   scenes=[{'nodes': [0]}], scene=0)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix('.tmp')
    glb.save(temporary)
    temporary.replace(output)
    return {'vertices': vertices, 'triangles': triangles}


def export(catalog_path: Path, private: Path, report=print) -> dict:
    """Write every hand prop whose model resolves; returns the hand_props.json content."""
    from tools.asset_pipeline.dynamic_props import load_catalog
    tables = json.loads((private/'living_world/tables.json').read_text(encoding='utf-8'))
    templates, textures = load_catalog(catalog_path)
    props = {}
    for key, ids in model_ids(tables).items():
        template = templates.get(ids['template']) if ids['template'] else None
        if template is None:
            props[key] = {'model': ids['model'], 'error': f"no DMO template {ids['template']}"}
            report(f"Hand prop {key} ({ids['model']}) unavailable: no DMO template {ids['template']}")
            continue
        try:
            stats = write_glb(template, textures, private/'living_world/hand_props'/f'{key}.glb')
        except (ValueError, KeyError) as error:
            props[key] = {'model': ids['model'], 'error': str(error)}
            report(f'Hand prop {key} unavailable: {error}')
            continue
        props[key] = {'model': ids['model'], 'template': ids['template'], 'file': f'hand_props/{key}.glb',
                      'characteristics': template.get('characteristics'), **stats}
    out = {'version': VERSION, 'props': props}
    (private/'living_world/hand_props.json').write_text(json.dumps(out, indent=2), encoding='utf-8')
    return out
