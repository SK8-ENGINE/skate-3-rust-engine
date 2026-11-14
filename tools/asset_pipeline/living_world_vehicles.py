"""Living world vehicles (setup group ``livingworld``): the ambient car models and their data, read
from the user's disc at setup time.

Outputs (under ``<assets>/private/living_world/``):

``vehicles/<recipe>.glb``
    One glTF per vehicle recipe of ``livingworld.big`` (``recipe/vehicle/*.recipe``, schema 6.0,
    ``type="Vehicle"``), written straight from the retail rx2 arenas: the car's own 8-bone rig
    (``Vehicle_Root``, ``Chassis`` and the wheel bones, every one parented to the rig root at its bind
    pose), every part as one primitive per material instance (``Accessory`` = body, ``Equipment`` =
    windows, ``Misc`` = separate wheels of ``reda_car``), every LOD as its own mesh / node (``LOD0``,
    ``LOD1``). Materials carry ``extras.kind`` = ``vehicle_chassis`` or ``vehicle_glass`` (from the
    recipe's XML sibling), ``extras.attrib_material`` = the part's ``AttribulatorMaterialName`` (the
    vault material row retail binds, e.g. ``character.livingworld_vehicles``), the diffuse map (RGBA as
    stored), the normal map and ``extras.environment`` = the id of the environment cube (one shared
    256x256 DXT5 cube on every retail car), written once as ``vehicles/env_<id>.png``: the six faces
    stacked top to bottom in native order (+X, -X, +Y, -Y, +Z, -Z), rows not flipped, like the map
    cubes (``map_writer``). Space: the arena's own (metres, Y up, wheels on
    y = 0, +Z forward).

``vehicles.json``
    Everything an engine or a mod needs to place a car, keyed by stable retail names a mod can
    override or extend: ``models`` (``livingworld_models`` records with a vehicle recipe: recipe,
    glb, ``chassis_colours`` / ``secondary_colours`` palettes with stable ids ``<model>/chassis/<i>``,
    the record's size and wheel fields, mesh bounds and wheel bones measured from the arena),
    ``entities`` (``livingworld_entities`` under ``vehicles``: model, spec, driver, scoring record),
    ``census`` (vehicle census records -> categories -> entities -> recipes) and ``recipes`` (parts,
    LODs, materials, glb summary).

Entry points: :func:`vehicle_manifest`, :func:`export_models`, :func:`vehicle_doc`, :func:`export`.
"""
from __future__ import annotations

import io
import json
import re
import struct
from pathlib import Path

import numpy as np

VERSION = 3
ARCHIVE = 'data/content/livingworld.big'
RECIPE_DIR = 'data/content/recipe/vehicle/'
FLOAT16_4 = 0x001A2360  # Xenos vertex format of the car positions (4 x half float); the shared rx2 reader skips it
PART_ROLES = {'Accessory': 'body', 'Equipment': 'windows', 'Misc': 'wheels'}
VEHICLE_MODEL_CATEGORY = 3  # livingworld_models.category: eLivingWorldModelCategory 3 = vehicles [data]


# ---------------------------------------------------------------- recipes

def material_types(xml: bytes) -> dict[str, str]:
    """``{material id (16 hex digits): type}`` from a recipe's XML sibling (``<mat id=... type=...>``)."""
    text = xml.decode('latin-1')
    return {m.group(1).lower().rjust(16, '0'): m.group(2)
            for m in re.finditer(r'<mat\s+id="0x([0-9a-fA-F]+)"\s+type="([^"]+)"', text)}


def vehicle_manifest(game_root: Path) -> dict:
    """Every vehicle recipe with its arenas / textures checked and its material types."""
    from tools.owned_game.big import BigArchive
    from .living_world import parse_recipe
    big = BigArchive(Path(game_root)/ARCHIVE)
    paths = {e.path.lower(): e for e in big.entries}
    recipes, errors, used = {}, [], set()
    for entry in sorted(big.entries, key=lambda e: e.path):
        path = entry.path.lower()
        if not (path.startswith(RECIPE_DIR) and path.endswith('.recipe')):
            continue
        stem = Path(entry.path).stem
        try:
            recipe = parse_recipe(big.read(entry))
        except (ValueError, struct.error, UnicodeDecodeError) as error:
            errors.append(f'{entry.path}: {error}')
            continue
        xml = paths.get(f'{RECIPE_DIR}{stem}.xml')
        kinds = material_types(big.read(xml)) if xml else {}
        missing = []
        for part in recipe['parts']:
            for lod in part['lods']:
                lod['arena'] = f"data/content/vehicle/model/{recipe['name']}/{part['slot']}/0x{lod['model']}.rx2"
                if lod['arena'].lower() not in paths:
                    missing.append(lod['arena'])
                for instance in lod.get('instances', [lod]):
                    for tid in instance['textures'].values():
                        texture = f'data/content/vehicle/texture/0x{tid}.rx2'
                        used.add(texture)
                        if texture not in paths:
                            missing.append(texture)
        recipes[stem] = {**recipe, 'kind': 'vehicle', 'xml': xml is not None, 'material_types': kinds,
                         'missing': sorted(set(missing))}
    textures = sorted(p for p in paths if p.startswith('data/content/vehicle/texture/'))
    return {'version': VERSION, 'archive': ARCHIVE, 'recipes': recipes, 'errors': errors,
            'textures': len(textures), 'unreferenced_textures': [t for t in textures if t not in used]}


# ---------------------------------------------------------------- models

def half_positions(data: bytes, mesh: dict, rx2) -> np.ndarray:
    """Positions of a mesh whose POSITION element is FLOAT16_4 (the car arenas): the vertex buffer is
    the raw GPU buffer whose size matches the descriptor (as the shared reader matches them)."""
    element = next((e for e in mesh.get('vertex_elements', []) if e['usage_name'] == 'POSITION'), None)
    if element is None or element['format'] != FLOAT16_4:
        raise ValueError(f"unsupported position format {element and hex(element['format'])}")
    stride, count = mesh['stride'], mesh['vertex_count']
    size = count * stride + mesh.get('vb_padding', 0)
    sections = rx2.parse_sections(data, rx2.parse_header(data))
    raws = sorted((s for s in sections if s['type_code'] == rx2.TYPE_RAW_BUFFER), key=lambda s: s['offset'])
    buffer = next((s for s in raws if s['size'] == size), raws[0] if raws else None)
    if buffer is None:
        raise ValueError('no vertex buffer')
    start = buffer['file_offset'] + element['offset']
    rows = np.frombuffer(data, dtype=np.uint8, count=stride * (count - 1) + 8, offset=start)
    raw = np.lib.stride_tricks.as_strided(rows, shape=(count, 8), strides=(stride, 1)).copy()
    return raw.view('>f2').astype(np.float64)[:, :3]


def write_glb(recipe: dict, arena_path, texture_path, output: Path, rx2) -> dict:
    """Write one vehicle recipe (see the module docstring). Returns bones, LOD vertex / triangle
    counts, mesh bounds and the wheel bone positions (bind pose, metres)."""
    from tools.asset_pipeline.character_glb import Glb
    from tools.asset_pipeline.living_world_models import _normal_png
    from PIL import Image
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/utt'))
    import mdl_parser
    glb = Glb()
    parsed, attribs = {}, {}
    for part in recipe['parts']:
        for level, lod in enumerate(part['lods']):
            path = arena_path(lod)
            data = rx2.parse_rx2(str(path))
            raw = Path(path).read_bytes()
            names = {m.value for m in mdl_parser.parse_rx2(raw).materials if m.kind == 'AttribulatorMaterialName'}
            attribs[(part['slot'], level)] = names.pop() if len(names) == 1 else None
            for mesh in data['meshes']:
                if mesh.get('indices') and not mesh.get('positions') and mesh.get('vertex_count'):
                    mesh['positions'] = half_positions(raw, mesh, rx2)
            parsed[(part['slot'], level)] = data
    main = max(parsed.values(), key=lambda d: len(d['bones']))
    bones = main['bones']
    names = [b['name'] for b in bones]
    index = {n: i for i, n in enumerate(names)}
    worlds = {b['name']: np.asarray(b['bind_matrix'], dtype=np.float64) for b in bones}
    for data in parsed.values():  # bones a secondary part has and the body lacks (sedan_4door_02 windows: lod_high)
        for bone in data['bones']:
            if bone['name'] not in index:
                index[bone['name']] = len(names)
                names.append(bone['name'])
                worlds[bone['name']] = np.asarray(bone['bind_matrix'], dtype=np.float64)
    nodes = [{'name': n, 'matrix': worlds[n].ravel().tolist()} for n in names]
    skinned = bool(names)
    inverse = glb.accessor([np.linalg.inv(worlds[n]).ravel() for n in names], 'MAT4') if skinned else None
    kinds = recipe.get('material_types', {})
    materials, meshes, lo, hi = {}, [], np.full(3, np.inf), np.full(3, -np.inf)
    stats = {'bones': len(names), 'lods': []}
    levels = max(len(p['lods']) for p in recipe['parts'])
    for level in range(levels):
        primitives, vertices, triangles = [], 0, 0
        for part in recipe['parts']:
            if len(part['lods']) <= level:
                continue
            lod = part['lods'][level]
            data = parsed[(part['slot'], level)]
            instances = lod.get('instances', [lod])
            raw = [m for m in data['meshes'] if len(m.get('positions', [])) and m.get('indices')]
            if len(raw) == 1 and len(instances) > 1 and all(i == instances[0] for i in instances):
                instances = instances[:1]  # reda_car wheels: one mesh, two identical material instances [data]
            if len(raw) != len(instances):
                raise ValueError(f"{recipe['name']}/{part['slot']} LOD{level}: {len(raw)} meshes for "
                                 f"{len(instances)} material instances")
            part_names = [b['name'] for b in data['bones']]
            for mesh, instance in zip(raw, instances):
                positions = np.asarray(mesh['positions'], dtype=np.float64)
                if level == 0:
                    lo, hi = np.minimum(lo, positions.min(0)), np.maximum(hi, positions.max(0))
                normals = np.asarray(mesh['normals'], dtype=np.float64)
                normals /= np.maximum(np.linalg.norm(normals, axis=1, keepdims=True), 1e-20)
                attributes = {'POSITION': glb.accessor(positions, 'VEC3', bounds=True),
                              'NORMAL': glb.accessor(normals, 'VEC3'),
                              'TEXCOORD_0': glb.accessor(mesh['uvs'], 'VEC2')}
                if skinned and mesh.get('skin_bones'):
                    joints = np.asarray([[index[part_names[b]] for b in q] for q in mesh['skin_bones']], dtype=np.uint16)
                    weights = np.asarray(mesh['skin_weights'], dtype=np.float32)
                    total = weights.sum(axis=1, keepdims=True)
                    if (total <= 0).any():
                        raise ValueError(f"{recipe['name']}/{part['slot']}: vertex without skin weight")
                    attributes['JOINTS_0'] = glb.accessor(joints, 'VEC4', 5123)
                    attributes['WEIGHTS_0'] = glb.accessor(weights / total, 'VEC4')
                attrib = attribs[(part['slot'], level)]
                key = (instance['material'], tuple(sorted(instance['textures'].items())), attrib)
                if key not in materials:
                    textures = instance['textures']
                    if 'diffuse' not in textures:
                        raise ValueError(f"{recipe['name']}/{part['slot']}: no diffuse texture")
                    buffer = io.BytesIO()
                    Image.open(texture_path(textures['diffuse'])).convert('RGBA').save(buffer, format='PNG')
                    kind = kinds.get(instance['material'] or '', 'unknown')
                    material = {'name': f"{kind}_{instance['material']}",
                                'pbrMetallicRoughness': {'baseColorTexture': glb.texture(buffer.getvalue()),
                                                         'metallicFactor': 0.0, 'roughnessFactor': 0.5},
                                'alphaMode': 'OPAQUE',
                                'extras': {'kind': kind, 'material_id': instance['material'],
                                           'attrib_material': attrib,
                                           'environment': textures.get('environment')}}
                    if attrib:
                        stats.setdefault('attrib_materials', {}).setdefault(kind, set()).add(attrib)
                    if textures.get('environment'):
                        stats.setdefault('environments', set()).add(textures['environment'])
                    if 'normal' in textures:
                        material['normalTexture'] = glb.texture(_normal_png(texture_path(textures['normal'])))
                    glb.doc['materials'].append(material)
                    materials[key] = len(glb.doc['materials']) - 1
                primitives.append({'attributes': attributes, 'material': materials[key],
                                   'indices': glb.accessor(np.asarray(mesh['indices'], dtype=np.uint32), 'SCALAR', 5125),
                                   'extras': {'slot': part['slot'], 'role': PART_ROLES.get(part['slot'], part['slot'].lower()),
                                              'kind': kinds.get(instance['material'] or '', 'unknown')}})
                vertices += len(positions)
                triangles += len(mesh['indices']) // 3
        if primitives:
            meshes.append({'name': f'LOD{level}', 'primitives': primitives})
            stats['lods'].append({'vertices': vertices, 'triangles': triangles})
    lod_nodes = []
    for level in range(len(meshes)):
        node = {'name': f'LOD{level}', 'mesh': level}
        if skinned:
            node['skin'] = 0
        lod_nodes.append(len(nodes))
        nodes.append(node)
    root = len(nodes)
    nodes.append({'name': recipe['name'], 'children': list(range(len(names))) + lod_nodes})
    glb.doc.update(nodes=nodes, meshes=meshes, scenes=[{'nodes': [root]}], scene=0,
                   extras={'recipe': recipe['name'], 'kind': 'vehicle', 'lods': len(meshes)})
    if skinned:
        glb.doc['skins'] = [{'joints': list(range(len(names))), 'inverseBindMatrices': inverse}]
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix('.tmp')
    glb.save(temporary)
    temporary.replace(output)
    stats['attrib_materials'] = {k: sorted(v) for k, v in sorted(stats.get('attrib_materials', {}).items())}
    stats['environments'] = sorted(stats.get('environments', ()))
    stats['bounds'] = [[_r(v) for v in lo], [_r(v) for v in hi]]
    stats['wheels'] = {n: [_r(v) for v in worlds[n][3, :3]] for n in names if 'wheel' in n.lower()}
    return stats


def _r(value: float) -> float:
    return round(float(value), 4)


def recipe_grab_splines(recipe: dict, read) -> list[dict]:
    """The skitch grab splines of a recipe: the first part arena (recipe order) that has GRABDATA
    (retail `82C2A8C8`; b35 section 6). A recipe without one has none (no skitching on it)."""
    from .grab_data import grab_splines
    for part in recipe['parts']:
        for lod in part['lods']:
            found = grab_splines(read(lod['arena']))
            if found:
                return [{'points': s['control_points'], 'direction': s['direction'], 'bounds': s['bounds'],
                         'flags': s['flags']} for s in found]
    return []


def recipe_grind_rails(recipe: dict, read) -> list[dict]:
    """The grind rails of a recipe, in model space: the same part arena as the grab splines (car setup 82C3CCD8
    takes both lists from one definition, `82C2A8C8` -> +136: rail count +8, list +24), i.e. that arena's EB0004
    rails. The definition's rail count must equal them (true for every disc car); none without a definition."""
    from .dynamic_props import decode_grind_splines, sections
    from .grab_data import grab_splines
    for part in recipe['parts']:
        for lod in part['lods']:
            raw = read(lod['arena'])
            if not grab_splines(raw):
                continue
            lists = [struct.unpack_from('>I', raw, offset + 8)[0] for offset, _, _, _, _, kind in sections(raw) if kind == 0xEB001B]
            rails = decode_grind_splines(raw)
            if len(lists) > 1:
                raise ValueError('Car part arena with two rail lists')
            if (lists[0] if lists else 0) != len(rails):
                raise ValueError('Car rail list does not match its rail section')
            return rails
    return []


def recipe_collision(recipe: dict, read) -> dict | None:
    """The car body's retail physics collision in model space (research dmo-physics-collision.md: the ``Accessory``
    part arena holds one 0x00080001 volume over a ClusteredMesh, edge codes and surfaces): what the board and skater
    meet on a car, in place of a box from the model bounds."""
    from .dynamic_props import collision_from_volume, sections
    for part in recipe['parts']:
        if part['slot'] != 'Accessory':
            continue
        for lod in part['lods']:
            raw = read(lod['arena'])
            table = sections(raw)
            for index, entry in enumerate(table):
                if entry[5] == 0x00080001:
                    shape = collision_from_volume(raw, table, index)
                    if shape:
                        return shape
    return None


def decode_cube(parser, data: bytes) -> np.ndarray:
    """The six base faces of a cube texture rx2 as one (6 x size, size, 4) RGBA strip. The vendored
    decoder takes only DXT1 / A8R8G8B8 / B5G6R5 cubes (``tools/vendor`` is fingerprinted, so the DXT5
    case lives here): each face is a tiled slice of 32-block-aligned rows, 4 KiB aligned."""
    rx2 = parser.RX2File(data)
    rx2.parse()
    entry = next((e for e in rx2.entries if e.is_texture), None)
    if entry is None:
        raise ValueError('no texture in cube rx2')
    hdr = rx2.data[entry.info_offset:entry.info_offset + 52]
    fmt, size = hdr[35], int.from_bytes(hdr[36:40], 'big')
    width, height = (size & 0x1FFF) + 1, ((size >> 13) & 0x1FFF) + 1
    if (int.from_bytes(hdr[48:52], 'big') >> 9) & 3 != 3 or width != height or fmt != parser.FMT_DXT5:
        raise ValueError(f'not a square DXT5 cube: {width}x{height} fmt={fmt:#x}')
    blocks = ((width + 3) // 4 + 31) & ~31
    stride = (blocks * blocks * 16 + 4095) & ~4095
    raw = rx2.data[rx2.data_base + entry.data_offset:][:entry.buffer_size]
    if len(raw) < 6 * stride:
        raise ValueError('truncated cube faces')
    faces = [np.frombuffer(parser._decode_texture_data(raw[i * stride:(i + 1) * stride], width, height, fmt, hdr[28]),
                           dtype=np.uint8).reshape(height, width, 4) for i in range(6)]
    return np.concatenate(faces, 0)


def export_models(game_root: Path, manifest: dict, output: Path, work: Path, report=print) -> dict:
    """Convert every recipe of ``manifest``. Returns {recipe: {status, file?, stats?, error?}}; a
    failed recipe does not stop the others."""
    from tools.extract_default_skater import decode_texture, import_rx2_parser
    from tools.asset_pipeline.retail_character import RX2
    from tools.owned_game.big import BigArchive
    big = BigArchive(Path(game_root)/manifest['archive'])
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
            decode_texture(parser, extract(f'data/content/vehicle/texture/0x{tid}.rx2'), dest)
        return dest

    results = {}
    environments = {}
    for name, recipe in sorted(manifest['recipes'].items()):
        try:
            stats = write_glb(recipe, lambda lod: extract(lod['arena']), texture, output/f'{name}.glb', RX2)
            for tid in stats.get('environments', []):
                if tid not in environments:
                    try:
                        from PIL import Image
                        Image.fromarray(decode_cube(parser, extract(f'data/content/vehicle/texture/0x{tid}.rx2').read_bytes())).save(output/f'env_{tid}.png')
                        environments[tid] = f'vehicles/env_{tid}.png'
                    except (ValueError, KeyError, struct.error) as error:
                        environments[tid] = None  # drawn without reflections
                        report(f'Living world vehicle environment {tid} unavailable: {error}')
            results[name] = {'status': 'ready', 'file': f'vehicles/{name}.glb', **stats,
                             'grab_splines': recipe_grab_splines(recipe, lambda arena: extract(arena).read_bytes()),
                             'grind_splines': recipe_grind_rails(recipe, lambda arena: extract(arena).read_bytes()),
                             'collision': recipe_collision(recipe, lambda arena: extract(arena).read_bytes())}
        except (ValueError, KeyError, RuntimeError, IndexError, struct.error, OSError) as error:
            if isinstance(error, OSError) and not isinstance(error, FileNotFoundError):
                raise  # disk full / permissions abort setup
            results[name] = {'status': 'unavailable', 'error': str(error)}
            report(f'Living world vehicle {name} unavailable: {error}')
    results['__environments__'] = {k: v for k, v in sorted(environments.items()) if v}
    return results


# ---------------------------------------------------------------- vehicles.json

def _colours(values) -> list[list[float]]:
    return [[v['x'], v['y'], v['z'], v['w']] for v in values or [] if isinstance(v, dict)]


def material_params(collections: list[dict] | None, names, table=None) -> dict:
    """``{name: {'params': m_params rows}}`` for each ``AttribulatorMaterialName`` (``class.key`` ->
    vault class ``material_<class>``, row ``key``, inheritance applied; retail ``sub_827E4F58``
    splits the name the same way). Names whose row is missing are left out. ``table`` reuses a built
    ``environment.Collections`` of ``collections``."""
    from .environment import Collections, key_hash
    if table is None:
        if not collections:
            return {}
        table = Collections({'collections': collections})
    out = {}
    for name in sorted(names):
        family, _, key = name.partition('.')
        try:
            fields, _ = table.resolve('material_' + family, key)
        except KeyError:
            continue
        rows = fields.get(key_hash('m_params'), {}).get('array', {}).get('items', [])
        if rows:
            out[name] = {'params': [[_r(v) for v in struct.unpack('>4f', bytes.fromhex(r))] for r in rows]}
    return out


def vehicle_doc(doc: dict, manifest: dict, census_records: list[str], collections: list[dict] | None = None) -> dict:
    """vehicles.json content from the exported tables (``living_world.tables``) and the manifest."""
    from .living_world import census_chain
    c = doc['classes']
    models = {}
    for key, record in sorted(c['livingworld_models'].items()):
        f = record['fields']
        recipe = f.get('recipe')
        if f.get('category') != VEHICLE_MODEL_CATEGORY or not recipe:
            continue
        built = manifest['recipes'].get(recipe, {}).get('glb', {})
        chassis, secondary = _colours(f.get('chassis_colours')), _colours(f.get('secondary_colours'))
        models[key] = {
            'recipe': recipe, 'parent': record['parent'],
            'glb': built.get('file') if built.get('status') == 'ready' else None,
            'chassis_colours': chassis, 'secondary_colours': secondary,
            'palette_ids': {'chassis': [f'{key}/chassis/{i}' for i in range(len(chassis))],
                            'secondary': [f'{key}/secondary/{i}' for i in range(len(secondary))]},
            'size_hint': f.get('Hash_F983F2518B335286'), 'wheel_hint': f.get('Hash_FD7A66142F16B9CC'),
            'mesh_bounds': built.get('bounds'), 'wheel_bones': built.get('wheels'),
            'grab_splines': built.get('grab_splines', []), 'grind_splines': built.get('grind_splines', []),
            # The retail chassis collision (the game fell back to a bounds box without it: the box's rear wall
            # sat between a skitcher's hands and shoulders, 2026-10-16).
            'collision': built.get('collision')}
    entities = {}
    for key, record in sorted(c['livingworld_entities'].items()):
        f = record['fields']
        if 'driver' not in f and 'spec' not in f:
            continue
        model = (f.get('model') or {}).get('key')
        entities[key] = {'parent': record['parent'], 'model': model,
                         'recipe': models.get(model, {}).get('recipe'),
                         'spec': (f.get('spec') or {}).get('key'), 'driver': (f.get('driver') or {}).get('key'),
                         'scoring': (f.get('scoring') or {}).get('key'), 'ai_graph': f.get('ai_graph')}
    census = {}
    for record in census_records:
        if record in c['livingworld_census']:
            chain = census_chain(doc, record)
            census[record] = {'max_population': chain['max_population'], 'group': chain['group'],
                              'vehicle_extra': c['livingworld_census'][record]['fields'].get('vehicle_extra'),
                              'categories': [{'category': k['category'], 'weight': k['weight'],
                                              'entities': [e['entity'] for e in k['entities']]}
                                             for k in chain['categories']]}
    recipes = {name: {'parts': [{'slot': p['slot'], 'role': PART_ROLES.get(p['slot'], p['slot'].lower()),
                                 'lods': len(p['lods'])} for p in r['parts']],
                      'material_types': r['material_types'], 'glb': r.get('glb')}
               for name, r in sorted(manifest['recipes'].items())}
    # Retail material rows of the car materials (vehicle_defaultPS / vehicle_glass_defaultPS m_params),
    # by material name, and the name each material kind used (the default for a mod GLB without one).
    kinds: dict[str, set] = {}
    for r in manifest['recipes'].values():
        for kind, names in (r.get('glb') or {}).get('attrib_materials', {}).items():
            kinds.setdefault(kind, set()).update(names)
    materials = material_params(collections, {n for names in kinds.values() for n in names})
    material_kinds = {kind: sorted(names)[0] for kind, names in sorted(kinds.items()) if len(names) == 1}
    return {'version': VERSION, 'models': models, 'entities': entities, 'census': census, 'recipes': recipes,
            'materials': materials, 'material_kinds': material_kinds,
            # Environment cube id (GLB material extras ``environment``) -> strip PNG.
            'environments': manifest.get('environments', {}),
            'specs': sorted(c.get('livingworld_vehicle_characteristics', {})),
            'drivers': sorted(c.get('livingworld_vehicle_drivers', {}))}


def validate(doc: dict, vehicles: dict, root: Path) -> list[str]:
    """Every painted vehicle census record resolves to entities with a model, a built car, a spec and a driver."""
    problems = []
    specs = doc['classes'].get('livingworld_vehicle_characteristics', {})
    drivers = doc['classes'].get('livingworld_vehicle_drivers', {})
    for record, census in vehicles['census'].items():
        if not census['categories']:
            problems.append(f'vehicle census {record}: no categories')
        for category in census['categories']:
            if not category['entities']:
                problems.append(f"vehicle census {record}: category {category['category']} has no entities")
            for name in category['entities']:
                entity = vehicles['entities'].get(name)
                if entity is None:
                    problems.append(f'vehicle census {record}: entity {name} is not a vehicle entity')
                    continue
                model = vehicles['models'].get(entity['model'])
                if model is None:
                    problems.append(f"vehicle entity {name}: model {entity['model']} has no vehicle recipe")
                elif not model['glb'] or not (root/model['glb']).is_file():
                    problems.append(f"vehicle entity {name}: car {model['recipe']} not built")
                elif not model['chassis_colours']:
                    problems.append(f"vehicle model {entity['model']}: no chassis colours")
                if entity['spec'] not in specs:
                    problems.append(f"vehicle entity {name}: spec {entity['spec']} missing")
                if entity['driver'] not in drivers:
                    problems.append(f"vehicle entity {name}: driver {entity['driver']} missing")
    return sorted(set(problems))


def export(game_root: Path, output: Path, work: Path, doc: dict, census_records: list[str], report=print,
           collections: list[dict] | None = None) -> dict:
    """Build the car models and ``vehicles.json`` into ``output`` (``living_world/``). Returns a report
    (counts and warnings); raises when the vehicle census does not resolve."""
    report('Reading vehicle recipes')
    manifest = vehicle_manifest(game_root)
    warnings = list(manifest['errors'])
    report('Building vehicle models')
    built = export_models(game_root, manifest, output/'vehicles', work/'living_world_vehicles', report)
    manifest['environments'] = built.pop('__environments__', {})
    for name, result in built.items():
        manifest['recipes'][name]['glb'] = result
        if result['status'] != 'ready':
            warnings.append(f"vehicle {name}: {result['error']}")
    vehicles = vehicle_doc(doc, manifest, census_records, collections)
    (output/'vehicles.json').write_text(json.dumps(vehicles, indent=1), encoding='utf-8')
    problems = validate(doc, vehicles, output)
    if problems:
        raise ValueError('living world vehicles do not resolve: ' + '; '.join(problems[:5]))
    return {'recipes': len(manifest['recipes']), 'ready': sum(1 for r in built.values() if r['status'] == 'ready'),
            'models': len(vehicles['models']), 'entities': len(vehicles['entities']),
            'unreferenced_textures': len(manifest['unreferenced_textures']), 'warnings': warnings}
