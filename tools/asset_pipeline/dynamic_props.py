"""Export authored district DMO locators and their referenced model geometry.

This preserves initial placement, not DMO simulation. Native 0x825876D0 reads
the locator matrix and template ID at +112; RX2 EB001D records are 128 bytes.
No name matching, random placement, ground snapping or collision synthesis.

Geometry is exported once per template and each placement becomes a MOBJ
schema 4 instance record (identity, name, shared index range, row-vector
affine transform). The runtime spawns one entity per instance; the JSON
sidecar retains full IDs and matrices.
"""
import argparse
import copy
import hashlib
import json
import struct
import sys
import tempfile
from pathlib import Path

import numpy as np
from .map_writer import write
from tools.owned_game.big import BigArchive


def sections(raw):
    if raw[:7] != b'\x89RW4xb2':
        raise ValueError('Expected Xbox RX2')
    count = struct.unpack_from('>I', raw, 32)[0]
    table = struct.unpack_from('>I', raw, 48)[0]
    if table + count*24 > len(raw):
        raise ValueError('Truncated RX2 section table')
    return [struct.unpack_from('>6I', raw, table+i*24) for i in range(count)]


def matrix(raw, at):
    value = np.array(struct.unpack_from('>16f', raw, at)).reshape(4, 4)
    if not np.isfinite(value).all() or not np.allclose(value[:, 3], [0, 0, 0, 1], atol=1e-5):
        raise ValueError('Invalid DMO affine matrix')
    if abs(np.linalg.det(value[:3, :3])) < 1e-8:
        raise ValueError('Singular DMO matrix')
    return value


def records(raw, kind, stride):
    for offset, _, size, _, _, type_id in sections(raw):
        if type_id != kind:
            continue
        if offset+size > len(raw) or size < 32:
            raise ValueError('Invalid DMO section extent')
        count = struct.unpack_from('>I', raw, offset+4)[0]
        start, strings = struct.unpack_from('>2I', raw, offset+12)
        if start != 32 or strings != start+count*stride or strings > size:
            raise ValueError('Unexpected DMO record layout')
        for index in range(count):
            yield offset, offset+start+index*stride, size


def locators(raw):
    result = []
    for base, at, size in records(raw, 0xEB001D, 128):
        name_offset = struct.unpack_from('>I', raw, at+124)[0]
        if not 32 <= name_offset < size:
            raise ValueError('Invalid DMO locator name offset')
        end = raw.index(b'\0', base+name_offset, base+size)
        result.append(dict(instance_id=f'{struct.unpack_from(">Q", raw, at+96)[0]:016X}',
            locator_id=f'{struct.unpack_from(">Q", raw, at+104)[0]:016X}',
            template_id=f'{struct.unpack_from(">Q", raw, at+112)[0]:016X}',
            matrix=matrix(raw, at).tolist(), name=raw[base+name_offset:end].decode('utf-8'),
            source_offset=at, bounds=np.array(struct.unpack_from('>8f', raw, at+64)).reshape(2, 4)[:, :3].tolist()))
    return result


def template_meshes(raw):
    """Resolve EB000D tInstance -> EB0001 model -> EB0023 mesh -> declaration."""
    from .grab_data import GRABDATA, section_splines
    table = sections(raw)
    result = {}
    rails = None
    # Grab splines by position (b48, validated against parkassets' single-template copies; the retail link is
    # not decoded): the GRABDATA section between a template's EB0001 model and the next template's model.
    model_indices = sorted(struct.unpack_from('>I', raw, at+128)[0] for _, at, _ in records(raw, 0xEB000D, 160))
    for _, at, _ in records(raw, 0xEB000D, 160):
        key = f'{struct.unpack_from(">Q", raw, at+104)[0]:016X}'
        index = struct.unpack_from('>I', raw, at+128)[0]
        if index >= len(table) or table[index][5] != 0xEB0001:
            raise ValueError('Unresolved DMO model reference')
        base, _, size, _, _, _ = table[index]
        start = struct.unpack_from('>I', raw, base+36)[0]
        count = struct.unpack_from('>H', raw, base+48)[0]
        if base+size > len(raw) or start+count*8 > size:
            raise ValueError('Invalid DMO model mesh table')
        mesh_info = []
        for j in range(count):
            mesh_index = struct.unpack_from('>I', raw, base+start+j*8)[0]
            if mesh_index >= len(table) or table[mesh_index][5] != 0xEB0023:
                raise ValueError('Unresolved DMO mesh reference')
            descriptor = struct.unpack_from('>I', raw, table[mesh_index][0]+40)[0]
            if descriptor >= len(table) or table[descriptor][5] != 0x200E9:
                raise ValueError('Unresolved DMO vertex declaration')
            mesh_info.append(table[descriptor][0])
        if key in result:
            raise ValueError('Duplicate DMO template ID')
        following = next((i for i in model_indices if i > index), len(table))
        grab = [i for i in range(index + 1, following) if table[i][5] == GRABDATA]
        if len(grab) > 1:
            raise ValueError('Two GRABDATA sections for one DMO template')
        splines = [dict(points=g['control_points'], direction=g['direction'], bounds=g['bounds'], flags=g['flags'])
                   for i in grab for g in section_splines(raw, table[i][0], table[i][2])]
        if rails is None:
            rails = decode_grind_splines(raw)
        result[key] = dict(mesh_info=mesh_info, matrix=matrix(raw, at), grab_splines=splines,
                          grind_splines=template_rails(raw, table, at, index, following, rails),
                          attach=template_attach(raw, table, at, index, following),
                          model_matrix=matrix(raw, base+struct.unpack_from('>I', raw, base+32)[0]),
                          characteristics=characteristics_key(raw, at), collision=template_collision(raw, table, at))
    return result


def template_collision(raw, table, at):
    """Retail physics collision of one EB000D part, in the part's model space, or None.

    Chain (research dmo-physics-collision.md section 2): tInstance +132 -> EB000A link, +8 -> 0x00040006 (assembly
    definition), +112 -> root volume 0x00080001 (type 6 aggregate) -> 0x00080002 array of 96 B volumes; a type 6
    entry whose child is a 0x00080006 ClusteredMesh carries the triangles with their RW edge codes and surfaces (the
    decoder the map collision uses). Primitive volumes (boxes etc.) are not exported yet. The other link form,
    20 bytes (0, count, 12, volume index..., 0), names root volumes directly with no assembly (research b437: the
    skate.Park rails and fences; likely the DMO templates that used to resolve to nothing). None = no ClusteredMesh
    found; the caller keeps the render mesh."""
    def u32(offset):
        return struct.unpack_from('>I', raw, offset)[0]
    def kind(index):
        return table[index][5] if index < len(table) else None
    link = u32(at+132)
    if kind(link) != 0xEB000A:
        return None
    base, _, size, _, _, _ = table[link]
    if size >= 16 and 1 <= u32(base+4) < 0xFFFFFFFF and u32(base+8) == 12:
        # The other link form (research b437; the skate.Park rails and fences): (0, count, 12, volume index..., 0),
        # the list at +12 naming root volumes directly, no assembly.
        shapes = [collision_from_volume(raw, table, u32(base+12+4*i)) for i in range(u32(base+4))
                  if 12+4*i+4 <= size and kind(u32(base+12+4*i)) == 0x00080001]
        shapes = [s for s in shapes if s]
        if not shapes:
            return None
        meshes = 0
        for s in shapes:
            s['vertex_keys'] = [[[m + meshes, c, i] for m, c, i in k] for k in s['vertex_keys']]
            meshes = 1 + max((m for k in s['vertex_keys'] for m, _, _ in k), default=meshes - 1)
        return {key: [v for s in shapes for v in s[key]] for key in ('triangles', 'edges', 'surfaces', 'units', 'vertex_keys')}
    assembly = u32(base+8)
    if kind(assembly) != 0x00040006:
        return None
    root = u32(table[assembly][0]+112)
    if kind(root) != 0x00080001:
        return None
    return collision_from_volume(raw, table, root)


def collision_from_volume(raw, table, root):
    """The ClusteredMesh triangles (edge codes, surfaces) under one RW collision volume section (aggregates walked)."""
    def u32(offset):
        return struct.unpack_from('>I', raw, offset)[0]
    meshes = []
    def walk(index, depth):
        if depth > 4 or index >= len(table):
            return
        base, _, size, _, _, type_id = table[index]
        if type_id == 0x00080006:
            meshes.append(bytes(raw[base:base+size]))
        elif type_id == 0x00080001:
            if u32(base+64) == 6:
                walk(u32(base+68), depth+1)
        elif type_id == 0x00080002:
            count, offset = u32(base+36), u32(base+48)
            for i in range(count):
                entry = base+offset+i*96
                if entry+96 > base+size:
                    break
                if u32(entry+64) == 6:
                    walk(u32(entry+68), depth+1)
    walk(root, 0)
    if not meshes:
        return None
    triangles, edges, surfaces, units, keys = [], [], [], [], []
    for m, data in enumerate(meshes):
        decoded, unit_list = decode_with_units(data)
        for t, cluster, (unit_type, sub, corners) in zip(decoded.triangles, decoded.triangle_cluster_indices, unit_list):
            triangles.append([list(t.a), list(t.b), list(t.c)])
            edges.append(list(t.edge_codes) if t.edge_codes is not None else None)
            surfaces.append(t.surface)
            units.append([unit_type, sub])
            keys.append([[m, cluster, i] for i in corners])
    return dict(triangles=triangles, edges=edges, surfaces=surfaces, units=units, vertex_keys=keys)


def decode_with_units(data):
    """docs/hails-additions/39-skatepark-layouts.md#fused-vertex-order"""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/university/tools/vanilla_map_extraction/tools'))
    import retail_collision_mesh as rcm
    units, original = [], rcm._triangulate_unit
    def recording(unit_type, indices):
        out = original(unit_type, indices)
        units.extend((unit_type, sub, list(corners)) for sub, corners in enumerate(out))
        return out
    rcm._triangulate_unit = recording
    try:
        decoded = rcm.decode_clustered_mesh(data)
    finally:
        rcm._triangulate_unit = original
    if len(units) != len(decoded.triangles):
        raise ValueError('ClusteredMesh unit record does not match its triangles')
    return decoded, units


def decode_grind_splines(raw):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/university/tools/vanilla_map_extraction/tools'))
    from retail_grind_splines import decode_grind_splines as decode
    return decode(bytes(raw))


def template_attach(raw, table, at, index, following):
    """The part's attach locator (row-vector 4x4, part model space), or None.

    Retail 82C4E128 walks the part's EB001B list (tInstance +136): point count +4, entry table +0x14, 8 B entries
    (key, subreference); the point whose u32 at +0x60 is 0 becomes the template's attach matrix (template+0x78), which
    the hand prop placement 82E3E1B0 applies inverted (research b204, b205). The points are the 0x70 B objects of
    the EB001E section in the part's block (matrix +0x00, two points +0x40 / +0x50, type +0x60). The subreference
    table is not decoded; like ``template_rails``, the section's count must match the list's."""
    link = struct.unpack_from('>I', raw, at+136)[0]
    if link == 0 or link >= len(table) or table[link][5] != 0xEB001B:
        return None
    count = struct.unpack_from('>I', raw, table[link][0]+4)[0]
    blocks = [i for i in range(index+1, following) if table[i][5] == 0xEB001E]
    if count == 0 or len(blocks) != 1:
        return None
    base, _, size, _, _, _ = table[blocks[0]]
    found, start = struct.unpack_from('>2I', raw, base)
    if found != count or start+found*0x70 > size:
        raise ValueError('DMO part point list does not match its point section')
    for k in range(found):
        at_point = base+start+k*0x70
        if struct.unpack_from('>I', raw, at_point+0x60)[0] == 0:
            return matrix(raw, at_point).tolist()
    return None


def template_rails(raw, table, at, index, following, rails):
    """Grind rails of one EB000D part, in the part's model space.

    Retail DMO setup 82C535C0 reads the part's EB001B list (tInstance +136):
    rail count +8, subreference list +24, inserted into the DMO rail map
    (+1796) that the grind provider 82C4C4E0 walks. The subreference table is
    not decoded; the list must cover exactly the rails of the one EB0004
    section in the part's block (true for every Downtown part)."""
    link = struct.unpack_from('>I', raw, at+136)[0]
    blocks = [i for i in range(index+1, following) if table[i][5] == 0xEB0004]
    if link == 0 and not blocks:
        return []  # No list: 82C535C0 skips a null +136.
    if link >= len(table) or table[link][5] != 0xEB001B:
        raise ValueError('Unresolved DMO part rail list')
    count = struct.unpack_from('>I', raw, table[link][0]+8)[0]
    if count == 0:
        if blocks:
            raise ValueError('DMO rail section outside the part rail list')
        return []
    if len(blocks) != 1:
        raise ValueError('DMO part rail list without one rail section')
    found = [rail for rail in rails if rail['section_index'] == blocks[0]]
    if len(found) != count:
        raise ValueError('DMO part rail list does not match its rail section')
    return found


def characteristics_key(raw, at):
    """Per-type DMO data record of one EB000D template.

    +112 is the vault class's 'default' collection id and +120 the record key
    of class livingworld_dynamicobject_characteristics: the record whose
    layout the DMO constructor 82C51E28 keeps at DMO+4380 -> +4 (+272
    restitution, +312 upright flag, +316..+328 friction pairs). Only the key is
    exported; the values stay in the installation's stock vault.
    """
    record = struct.unpack_from('>Q', raw, at+120)[0]
    return f'Hash_{record:016X}' if record else None


def transform_mesh(arrays, index, transform):
    # NpzFile.items() reads every array before the filter runs.
    values = {key: np.array(arrays[key], copy=True) for key in arrays if key.endswith('_'+str(index))}
    key = f'vertices_{index}'
    values[key] = (values[key] @ transform[:3, :3] + transform[3, :3]).astype('f4')
    normal_matrix = np.linalg.inv(transform[:3, :3]).T
    for prefix in ('normals', 'retail_normals'):
        key = f'{prefix}_{index}'
        if key in values:
            n = values[key] @ normal_matrix
            values[key] = (n/np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-20)).astype('f4')
    if np.linalg.det(transform[:3, :3]) < 0:
        values[f'faces_{index}'] = values[f'faces_{index}'][:, [0, 2, 1]]
    return values


def catalog(cache_roots):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/utt'))
    import rx2_parser
    from .backdrop import texture_groups
    templates, textures = {}, {}
    for root in cache_roots:
        m = json.loads((root/'manifest.json').read_text())
        for key, value in m['textures'].items():
            entry = dict(value)
            for path_key in ('rgba', 'png'):
                if path_key in entry:
                    entry[path_key] = str((root/entry[path_key]).resolve())
            if key in textures and Path(textures[key]['rgba']).read_bytes() != Path(entry['rgba']).read_bytes():
                raise ValueError('Conflicting DMO texture '+key)
            textures[key] = entry
        for model in m['models']:
            raw = (root/model['rx2']).read_bytes()
            table = rx2_parser.RX2File(raw)
            table.parse()
            roles = set(role for mesh in model['meshes'] for role in mesh['retail_texture_ids'])
            bindings = texture_groups(raw, table, roles)
            for key, definition in template_meshes(raw).items():
                meshes = [copy.deepcopy(mesh) for mesh in model['meshes'] if mesh['source_offsets']['mesh_info'] in definition['mesh_info']]
                for mesh in meshes:
                    # DMO resources have a high-bit namespace that display names
                    # omit. Bind the binary channel GUID; never strip that bit.
                    group = bindings[mesh['retail_material_group_index']]
                    mesh['retail_texture_ids'] = {role:f'0x{group[role]:016x}' for role in mesh['retail_texture_ids']}
                    mesh['texture_id'] = mesh['retail_texture_ids'].get('diffuse', mesh['retail_texture_ids'].get('transparent'))
                if len(meshes) != len(definition['mesh_info']):
                    raise ValueError('DMO model geometry omitted '+key)
                if key in templates:
                    raise ValueError('Conflicting DMO template '+key)
                templates[key] = dict(definition, meshes=meshes, npz=root/model['npz'], asset_id=model['asset_id'])
    return templates, textures


def save_catalog(cache_roots, output):
    """Build once per installation; JSON keeps this local cache non-executable."""
    templates, textures = catalog(cache_roots)
    serialised = {key: dict(value, matrix=value['matrix'].tolist(),
        model_matrix=value['model_matrix'].tolist(), npz=str(value['npz'].resolve()))
        for key, value in templates.items()}
    temporary = output.with_suffix('.new')
    temporary.write_text(json.dumps(dict(version=1, templates=serialised, textures=textures)), encoding='utf-8')
    temporary.replace(output)


def load_catalog(path):
    data = json.loads(path.read_text(encoding='utf-8'))
    if data['version'] != 1:
        raise ValueError('Unsupported DMO catalog version')
    for value in data['templates'].values():
        value['matrix'] = np.array(value['matrix'], dtype=np.float64)
        value['model_matrix'] = np.array(value['model_matrix'], dtype=np.float64)
        value['npz'] = Path(value['npz'])
    return data['templates'], data['textures']


def mobj_extension(records, ranges):
    """MOBJ schema 4 payload: schema 3 record plus a 12-float row-vector affine.

    `records` holds (locator, model_position, transform) tuples; `ranges` is
    the per-model (first index, index count) list reported by the map writer.
    """
    out = bytearray()
    u = lambda *v: out.extend(struct.pack('<'+'I'*len(v), *v))
    f = lambda *v: out.extend(struct.pack('<'+'f'*len(v), *v))
    u(len(records)); ids = set()
    for item, model, transform in records:
        identity = int(item['instance_id'], 16) & 0xFFFFFFFF
        if identity in ids:
            raise ValueError('Conflicting DMO identity '+item['instance_id'])
        ids.add(identity)
        if not np.isfinite(transform).all():
            raise ValueError('Non-finite DMO transform '+item['instance_id'])
        name = f"{item['template_id']}/{item['name']}".encode('utf-8')
        u(identity, len(name)); out.extend(name)
        f(*transform[3, :3])  # origin duplicates the affine translation
        first, count = ranges[model]
        u(first, count, 0, 0, 0)  # render range, collision range, no rails
        u(0, 0)  # no physics body, collision shape unused
        f(100., .55, .05, .05, .15, 1.)  # authored-style physics defaults
        u(1, 0)  # enable_sleep, initially_awake
        f(*transform[:3, :3].ravel(), *transform[3, :3])
    return bytes(out)


def hotpoint_props(instance_id, template_id, transform, hotpoints, classes):
    """Each hotpoint with a plugin class as one waypoint group (`82C4E128`) at the prop's initial placement: the
    model-frame point and facing axis (row 2) through the same row-vector transform as the prop's geometry."""
    result = []
    for index, h in enumerate(hotpoints):
        cls = classes.get(str(h['type']), classes.get('default'))
        if cls in (None, 'root'):
            continue
        position = (np.array([*h['position'], 1.0]) @ transform)[:3]
        facing = (np.array([*h['axes'][2], 0.0]) @ transform)[:3]
        length = float(np.hypot(facing[0], facing[2]))
        if length < 1e-6:
            raise ValueError('Hotpoint facing has no horizontal direction')
        result.append({'instance_id': instance_id, 'template_id': template_id, 'index': index, 'type': h['type'], 'class': cls,
                       'position': position.tolist(), 'facing': [float(facing[0] / length), 0.0, float(facing[2] / length)]})
    return result


def export(manifest_path, cache_roots, output, *, catalog_path=None):
    district = json.loads(manifest_path.read_text())
    templates, textures = catalog(cache_roots) if catalog_path is None else load_catalog(catalog_path)
    placements = {}
    for source in district['simulation_assets']:
        raw = (manifest_path.parent/source['rx2']).read_bytes()
        for item in locators(raw):
            item['source_asset'] = source['asset_id']
            item['source_sha256'] = hashlib.sha256(raw).hexdigest()
            key = item['instance_id']
            if key in placements and placements[key] != item:
                raise ValueError('Conflicting DMO locator '+key)
            placements[key] = item
    # Ped plugin hotpoints per template (parkassets.big, `hotpoint_data`), written by prepare_catalog beside the catalog.
    hotpoint_file = Path(catalog_path).parent/'hotpoints.json' if catalog_path is not None else None
    hotpoint_table = json.loads(hotpoint_file.read_text()) if hotpoint_file is not None and hotpoint_file.is_file() else {}
    report = write_placements(district['map_name'], district['district_name'], placements.values(), templates, textures,
                              output, hotpoint_table)
    return len(report['instances']), len(report['unresolved'])


def write_placements(map_name, district_name, placements, templates, textures, output, hotpoint_table=None, **extra):
    """Write `placements` (instance_id, template_id, matrix, name) of `templates` as a render-only MOBJ package at
    `output` plus its `.json` sidecar (instances, per-template collision / rails / types); returns the sidecar.
    The district movable props and the skate.Park pieces (park_layout.py) share it; `extra` joins the sidecar."""
    hotpoint_table = hotpoint_table or {}
    report = dict(map=map_name, instances=[], unresolved=[], simulation='initial placement only',
                  types={}, grab_splines={}, grind_splines={}, collision={}, hotpoints={}, hotpoint_classes={}, plugin_props=[])
    report.update(extra)
    with tempfile.TemporaryDirectory(prefix='skate-dmo-') as work:
        root = Path(work); models = []; records = []; used = set(); positions = {}
        for item in placements:
            template = templates.get(item['template_id'])
            if template is None:
                report['unresolved'].append(item)
                continue
            key = item['template_id']
            if key not in positions:
                # Template geometry stays in model space; instance transforms
                # carry the full placement so every locator shares one range.
                arrays = {}
                meshes = copy.deepcopy(template['meshes'])
                with np.load(template['npz'], allow_pickle=False) as original:
                    for mesh in meshes:
                        index = mesh['index']
                        for name in original.files:
                            if name.endswith('_'+str(index)):
                                arrays[name] = original[name]
                        used.update(mesh['retail_texture_ids'].values())
                path = f'{len(models)}.npz'; np.savez(root/path, **arrays)
                positions[key] = len(models)
                models.append(dict(asset_id=key, meshes=meshes, npz=path))
            transform = template['model_matrix'] @ template['matrix'] @ np.array(item['matrix'])
            records.append((item, positions[key], transform))
            report['instances'].append(dict(item, model_asset=template['asset_id'], meshes=len(template['meshes'])))
            if template.get('characteristics'):
                report['types'][key] = template['characteristics']
            if template.get('grab_splines'):
                report['grab_splines'][key] = template['grab_splines']
            if template.get('grind_splines'):
                report['grind_splines'][key] = template['grind_splines']
            if template.get('collision'):
                report['collision'][key] = template['collision']
            if key in hotpoint_table.get('hotpoints', {}):
                report['hotpoints'][key] = hotpoint_table['hotpoints'][key]
                report['hotpoint_classes'] = hotpoint_table['classes']
                report['plugin_props'].extend(hotpoint_props(item['instance_id'], key, transform, hotpoint_table['hotpoints'][key], hotpoint_table['classes']))
        if models:
            manifest = dict(map_name=map_name, district_name=district_name,
                models=models, textures={key:textures[key] for key in sorted(used)},
                normal_texture_policy=dict(excluded_texture_ids=[]), grind_splines=[])
            (root/'manifest.json').write_text(json.dumps(manifest))
            # Publish only a complete package; preserve the previous one on failure.
            temporary = output.with_suffix('.skate.new')
            try:
                write(root/'manifest.json', temporary, None, render_only=True,
                      extensions=lambda ranges: [(b'MOBJ', 4, mobj_extension(records, ranges))])
                temporary.replace(output)
            finally:
                temporary.unlink(missing_ok=True)
        else:
            output.unlink(missing_ok=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.with_suffix('.json').write_text(json.dumps(report, indent=2))
    return report


def prepare_catalog(game_root, work):
    vendor = Path(__file__).resolve().parents[1]/'vendor'
    sys.path.insert(0, str(vendor/'university/tools/vanilla_map_extraction/tools'))
    from prepare_hawaiian_dream import prepare
    archive = BigArchive(game_root/'data/content/worlddmo.big')
    archive.extract_entries(archive.entries, work/'raw')
    roots = []
    for stream in (work/'raw/data/content/world/dmo').iterdir():
        root = work/'cache'/stream.name
        prepare(stream_directory=stream, output_root=root, utt_root=vendor/'utt',
                district_name=stream.name, map_name=stream.name, raw_texture_cache=True)
        roots.append(root)
    save_catalog(roots, work/'catalog.json')
    # Ped plugin hotpoints (seats, bins, newspaper boxes) live in parkassets.big, not in worlddmo.big.
    from .hotpoint_data import CLASSES, parkassets_hotpoints
    (work/'hotpoints.json').write_text(json.dumps(dict(classes=CLASSES, hotpoints=parkassets_hotpoints(game_root))))
    return roots


def patch_collision(game_root, native_props, work):
    """Add the retail DMO collision (`template_collision`) to an installed `native-props/<map>.json` set without a full
    setup: every DMO stream of worlddmo.big is prepared, and each map's sidecar gets `collision` for the templates its
    instances use. Returns {map: templates written}."""
    roots = prepare_catalog(Path(game_root), Path(work))
    shapes = {}
    for root in roots:
        for path in Path(root).rglob('*.rx2'):
            raw = path.read_bytes()
            try:
                table = sections(raw)
            except ValueError:
                continue
            for _, at, _ in records(raw, 0xEB000D, 160):
                key = f'{struct.unpack_from(">Q", raw, at+104)[0]:016X}'
                if key not in shapes:
                    shape = template_collision(raw, table, at)
                    if shape:
                        shapes[key] = shape
    written = {}
    for sidecar in sorted(Path(native_props).glob('*.json')):
        report = json.loads(sidecar.read_text())
        used = {i.get('template_id') for i in report.get('instances', [])}
        report['collision'] = {k: shapes[k] for k in sorted(used) if k in shapes}
        sidecar.write_text(json.dumps(report, indent=2))
        written[sidecar.stem] = len(report['collision'])
    return written


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == 'patch-collision':
        # python -m tools.asset_pipeline.dynamic_props patch-collision <game root> <native-props dir> <work dir>
        print(patch_collision(*sys.argv[2:5]))
        sys.exit(0)
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--manifest', type=Path, required=True)
    p.add_argument('--cache', type=Path, nargs='+', required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    print('DMO placed/unresolved:', export(a.manifest, a.cache, a.output))
