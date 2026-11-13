"""skate.Park layouts (`.park`) and their placed static pieces (research b424, b425, b429 to b431).

A skate.Park world row (`dist_megapark_full`, `dist_startpark_progression`, ...) names a layout file in field
`Hash_79B248E8469DC5DA` (`skatepark\\MegaPark_Fullpark.park`, disc `miscload.big`, DLC packs their own BIG). The file
places recipe pieces on the district stream's shell; each piece id is a `<mod>` of the static recipe XML
(`recipe/static/static_db_*.xml`) whose first `<lod arenaid>` is the piece's RX2 (`static/model/static/<group>/`).
Retail keeps every piece as its own object (b430): it collides through a per-piece static set of the physics world
and grinds on its template's own splines posed by the piece matrix (b431), rebuilt when a layout is applied. So the
pieces are exported like the district DMO props (one template geometry, one MOBJ instance per piece,
`dynamic_props.write_placements`), never baked into the map.

Container (b425, big-endian): +0 version, +4..+16 four block lengths (static pieces, dynamic pieces, static ids,
dynamic ids), +20 u64 hash of the origin world row, blocks from +28. Block 1: u32 record format, u32 count, then
records of 36 bytes (format 1: position, quaternion x y z w, u64 piece id) or 48 bytes (format 2: plus RGB tint);
the block is allocated larger, so the next block starts at its length, never at count x stride. Block 2: u32 1,
u32 count, 40-byte records (position, quaternion, u64 piece id, u32 flag), the movable DMO props of the dynamic
recipe. Retail's static reader (82868318) skips id 0; the editor tail after block 4 is not read.

Output (`private/park-layouts/`): `<layout>.skate` + `<layout>.json` per layout with pieces, and `worlds.json`
{world row: {layout, stream}}. All of it is data: a mod adds or replaces a layout or a world's entry.
"""
import hashlib
import math
import json
import struct
import sys
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path

import numpy as np

LAYOUT_FIELD = 'Hash_79B248E8469DC5DA'
# The park worlds' shadow colours (research b443: 827FFFB0 reads them when the world's park bool is set; other
# worlds use (0.2, 0.2, 0.2), a code constant).
# The park worlds' projected lightmap (research b439 / b440: 8280FDC0 reads a texture name and x0, x1, z0, z1 from the
# world row; 828101C0 builds g_skateparkProjectedTransform from them). The textures: miscboot.big data/content/textures.
LIGHTMAP_NAME_FIELD = 'Hash_AA9212BDE5A603B4'
LIGHTMAP_BOUNDS_FIELDS = ('Hash_A3845713F3CC3AE8', 'Hash_3B8116674904A528', 'Hash_81A067E9478EC244', 'Hash_1101273D0D2882B5')
LIGHTMAPS = 'data/content/textures/'
PARK_WORLD_FIELD, DYNAMIC_SHADOW_FIELD, EXPONENTIAL_SHADOW_FIELD = ('Hash_0C54AD24155E3220', 'Hash_281597C51E312AFC',
                                                              'Hash_C26D37D18EF0D13E')
STATIC_RECIPE = 'data/content/recipe/static/'
DYNAMIC_RECIPE = 'data/content/recipe/dynamic/'
STATIC_MODELS = 'data/content/static/model/static/'
STATIC_TEXTURES = 'data/content/static/texture/'
# The movable pieces' DMO models and textures (research b435: the dynamic recipe's arena ids are DMO template ids).
DYNAMIC_MODELS = 'data/content/dynamic/model/'
DYNAMIC_TEXTURES = 'data/content/dynamic/texture/'


def parse(raw):
    """The static and dynamic pieces of one `.park` file."""
    if len(raw) < 28:
        raise ValueError('Truncated park layout')
    version, *lengths = struct.unpack_from('>5I', raw, 0)
    origin = struct.unpack_from('>Q', raw, 20)[0]
    starts = [28]
    for length in lengths:
        starts.append(starts[-1] + length)
    if starts[-1] > len(raw):
        raise ValueError('Park layout blocks run past the file')
    result = dict(version=version, origin_world_hash=f'{origin:016X}', static=[], dynamic=[], start=None)
    # The editor tail's first matrix (b425 section 4: v7 / v10-12 one, v2 / v3 the first of 8): the park's player start,
    # where retail places the skater on the park (user's video 2026-10-21: Stadium - Full starts on top of the big
    # ramp = MegaPark_Fullpark's (241.2, 37.1, -91.0)). Rows as stored, translation in row 3; its w lanes hold junk.
    if starts[-1] + 64 <= len(raw):
        m = [list(struct.unpack_from('>4f', raw, starts[-1] + 16*r)) for r in range(4)]
        if all(math.isfinite(v) for row in m for v in row[:3]) and all(abs(sum(v*v for v in m[r][:3]) - 1) < 0.01 for r in range(3)):
            result['start'] = [m[0][:3] + [0.0], m[1][:3] + [0.0], m[2][:3] + [0.0], m[3][:3] + [1.0]]
    if lengths[0]:
        record_format, count = struct.unpack_from('>2I', raw, starts[0])
        stride = {1: 36, 2: 48}.get(record_format)
        if stride is None or 8 + count*stride > lengths[0]:
            raise ValueError(f'Unsupported park static block (format {record_format}, {count} pieces)')
        for i in range(count):
            at = starts[0] + 8 + i*stride
            values = struct.unpack_from('>7fQ', raw, at)
            tint = list(struct.unpack_from('>3f', raw, at+36)) if record_format == 2 else [1.0, 1.0, 1.0]
            result['static'].append(dict(index=i, position=list(values[:3]), rotation=list(values[3:7]),
                                         piece=f'{values[7]:016X}', tint=tint))
    if lengths[1]:
        record_format, count = struct.unpack_from('>2I', raw, starts[1])
        if record_format != 1 or 8 + count*40 > lengths[1]:
            raise ValueError(f'Unsupported park dynamic block (format {record_format}, {count} pieces)')
        for i in range(count):
            values = struct.unpack_from('>7fQI', raw, starts[1] + 8 + i*40)
            result['dynamic'].append(dict(index=i, position=list(values[:3]), rotation=list(values[3:7]),
                                          piece=f'{values[7]:016X}', flag=values[8]))
    return result


def matrix(position, rotation):
    """Row-vector 4x4 (v @ m) of a piece: quaternion x, y, z, w (b425: unit length, about +Y for most pieces)."""
    x, y, z, w = rotation
    column = np.array([
        [1 - 2*(y*y + z*z), 2*(x*y - z*w), 2*(x*z + y*w)],
        [2*(x*y + z*w), 1 - 2*(x*x + z*z), 2*(y*z - x*w)],
        [2*(x*z - y*w), 2*(y*z + x*w), 1 - 2*(x*x + y*y)]])
    result = np.eye(4)
    result[:3, :3] = column.T
    result[3, :3] = position
    return result


def recipe(raw):
    """{piece id: name, RX2 arena id, cap.* values} of one recipe XML."""
    text = raw.decode('utf-8')
    root = ET.fromstring(text.split('?>', 1)[1] if text.lstrip().startswith('<?') else text)
    result = {}
    for mod in root.iter('mod'):
        lod = mod.find('lod')
        if lod is None or not lod.get('arenaid'):
            continue
        caps = {c.tag.removeprefix('cap.'): c.get('value', '') for c in mod.iter() if c.tag.startswith('cap.')}
        result[f"{int(mod.get('id'), 16):016X}"] = dict(name=mod.get('n', ''), arena=f"{int(lod.get('arenaid'), 16):016X}",
                                                         caps=caps)
    return result


class Sources:
    """The disc's park files and the player's packs, read in order: a later archive's recipe row, model or texture
    with the same id replaces the earlier one (the packs' recipes are numbered supersets of the disc's `_000`;
    no shared id differs between them, doc 39, so the order is moot)."""

    def __init__(self, archives):
        self.layouts, self.models, self.textures, self.static, self.dynamic, self.lightmaps = {}, {}, {}, {}, {}, {}
        for archive in archives:
            for entry in archive.entries:
                path = entry.path.replace('\\', '/').lower()
                if path.endswith('.park'):
                    self.layouts['skatepark/' + Path(path).name] = (archive, entry)
                elif path.endswith('.rx2') and path.startswith((STATIC_MODELS, DYNAMIC_MODELS)):
                    self.models[Path(path).stem.upper().removeprefix('0X')] = (archive, entry)
                elif path.endswith('.rx2') and path.startswith(LIGHTMAPS) and path.count('/') == LIGHTMAPS.count('/'):
                    self.lightmaps[Path(path).stem] = (archive, entry)
                elif path.endswith('.rx2') and path.startswith((STATIC_TEXTURES, DYNAMIC_TEXTURES)):
                    self.textures[int(Path(path).stem, 16)] = (archive, entry)
            for entry in sorted(archive.entries, key=lambda e: e.path.lower()):
                path = entry.path.replace('\\', '/').lower()
                if path.endswith('.xml') and path.startswith(STATIC_RECIPE):
                    self.static.update(recipe(archive.read(entry)))
                elif path.endswith('.xml') and path.startswith(DYNAMIC_RECIPE):
                    self.dynamic.update(recipe(archive.read(entry)))

    def read(self, table, key):
        archive, entry = table[key]
        return archive.read(entry)

    def layout(self, field_value):
        """The layout entry a world row's field names (`skatepark\\X.park`), or None."""
        key = field_value.replace('\\', '/').lower()
        return key if key in self.layouts else None


def world_layouts(collections):
    """{world row key: (layout field value, stream, shadow colours)} over every world row, the fields resolved through
    parents; the colours ({dynamic, exponential} RGB) are None unless the row's park bool is set."""
    from .environment import key_hash
    result = {}
    for (cls, key), row in collections.rows.items():
        if cls != key_hash('world'):
            continue
        fields, _ = collections.resolve('world', key)
        layout, stream = fields.get(key_hash(LAYOUT_FIELD)), fields.get(key_hash('WorldStream'))
        if layout and layout.get('data') and stream and stream.get('data'):
            result[row['key']] = (layout['data'], stream['data'], shadow_colours(fields), projected_lightmap(fields))
    return result


def shadow_colours(fields):
    """{dynamic, exponential}: the world row's g_DynamicShadowColour / g_ExponentialShadowColour RGB (big-endian
    Vector4 fields), None when the row is no park world (its park bool clear)."""
    from .environment import key_hash
    flag = (fields.get(key_hash(PARK_WORLD_FIELD)) or {}).get('data')
    if not flag or not int(flag[:2], 16):
        return None
    def colour(name):
        value = (fields.get(key_hash(name)) or {}).get('data')
        return list(struct.unpack('>3f', bytes.fromhex(value)[:12])) if value else [0.0, 0.0, 0.0]
    return dict(dynamic=colour(DYNAMIC_SHADOW_FIELD), exponential=colour(EXPONENTIAL_SHADOW_FIELD))


def projected_lightmap(fields):
    """{texture, bounds [x0, x1, z0, z1]} of a world row's projected lightmap, None without a name (the base row's is
    empty; non-park worlds have none)."""
    from .environment import key_hash
    name = (fields.get(key_hash(LIGHTMAP_NAME_FIELD)) or {}).get('data')
    if not name:
        return None
    def value(field):
        data = (fields.get(key_hash(field)) or {}).get('data')
        return struct.unpack('>f', bytes.fromhex(data))[0] if data else 0.0  # a missing field reads 0.0 (830D0850)
    return dict(texture=name, bounds=[value(f) for f in LIGHTMAP_BOUNDS_FIELDS])


def lightmap_texture(sources, name, path):
    """Decode one projected lightmap RX2 (512 x 512 DXT1, research b444) to RGBA at `path`; the manifest entry."""
    tools = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(tools/'vendor/utt'))
    import rx2_parser
    texture = rx2_parser.parse_rx2(sources.read(sources.lightmaps, name)).textures[0]
    path.write_bytes(texture.rgba)
    return dict(width=texture.width, height=texture.height, rgba=str(path))

def piece_template(sources, piece, work, recipe_rows=None):
    """Template geometry, textures and collision of one static piece, in the catalog shape `write_placements`
    reads (dynamic_props.catalog), or None when its RX2 is missing. `recipe_rows`: the dynamic recipe for a
    movable piece (default: the static one)."""
    from .backdrop import texture_groups, _texture
    from .dynamic_props import template_meshes
    tools = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(tools/'vendor/utt'))
    sys.path.insert(0, str(tools/'vendor/university/tools/vanilla_map_extraction/tools'))
    import mdl_parser
    import rx2_parser
    import prepare_hawaiian_dream as prep
    from retail_lightmap_uv import decode_decal_uvs, decode_lightmap_uvs
    from retail_texture_decode import B5G6R5_FORMAT_ID, decode_b5g6r5
    entry = (sources.static if recipe_rows is None else recipe_rows).get(piece)
    if entry is None or entry['arena'] not in sources.models:
        return None
    raw = sources.read(sources.models, entry['arena'])
    definitions = template_meshes(raw)
    if len(definitions) != 1:
        raise ValueError(f'Park piece {piece}: {len(definitions)} templates in its RX2')
    (_, definition), = definitions.items()
    model = mdl_parser.parse_rx2(raw)
    groups, bindings = prep._bind_material_groups_by_guid(
        raw, prep._group_material_parameters(model.materials), len(model.meshes), allow_import_order_fallback=False)
    table = rx2_parser.RX2File(raw)
    table.parse()
    channels = texture_groups(raw, table, prep.RETAIL_TEXTURE_CHANNELS)
    arrays, meshes, textures = {}, [], {}
    for i, mesh in enumerate(model.meshes):
        if mesh.source_offsets['mesh_info'] not in definition['mesh_info']:
            continue
        material, binding = prep._material_metadata(groups, i), bindings[i]
        group = channels[binding['group_index']]
        # Bind the binary channel GUIDs (the texture RX2 stems), as the DMO catalog does.
        roles = {role: f'0x{group[role]:016x}' for role in material['retail_texture_ids'] if role in group}
        meshes.append(dict(material, index=i, name=groups[i]['Name'][0], retail_texture_ids=roles,
            texture_id=roles.get('diffuse', roles.get('transparent')),
            retail_material_guid=f"0x{binding['material_guid']:016X}",
            retail_material_handle=f"0x{binding['material_handle']:08X}",
            retail_material_group_index=binding['group_index'], source_offsets=mesh.source_offsets))
        arrays[f'vertices_{i}'], arrays[f'faces_{i}'] = mesh.vertices, mesh.faces
        arrays[f'uvs_{i}'], arrays[f'normals_{i}'] = mesh.uvs, mesh.normals
        lightmap = decode_lightmap_uvs(raw, vertex_buffer_offset=mesh.source_offsets['vertex_buffer'],
            vertex_count=mesh.vertex_count, vertex_stride=mesh.vertex_stride, attributes=mesh.attributes)
        if lightmap is not None:
            arrays[f'lightmap_uvs_{i}'] = lightmap.values
        # The decal programs read their decal on its own UV set (TEXCOORD1), as the map exporter writes it
        # (prepare_hawaiian_dream); without it the decal falls back to the diffuse UVs and lands offset.
        decal = decode_decal_uvs(raw, vertex_buffer_offset=mesh.source_offsets['vertex_buffer'],
            vertex_count=mesh.vertex_count, vertex_stride=mesh.vertex_stride, attributes=mesh.attributes)
        if decal is not None:
            arrays[f'decal_uvs_{i}'] = decal.values
        for role, name in roles.items():
            guid = group[role]
            if name in textures or guid not in sources.textures:
                continue
            parsed = rx2_parser.parse_rx2(sources.read(sources.textures, guid))
            texture = _texture(parsed, guid)
            rgba = texture.rgba
            if texture.fmt_id == B5G6R5_FORMAT_ID:
                rgba = decode_b5g6r5(parsed.data[texture.data_offset:texture.data_offset+texture.buffer_size],
                                     texture.width, texture.height)
            path = work/(name + '.rgba')
            path.write_bytes(rgba)
            textures[name] = dict(width=texture.width, height=texture.height, rgba=str(path))
    if not meshes:
        return None
    npz = work/f'{piece}.npz'
    np.savez(npz, **arrays)
    return dict(definition, meshes=meshes, npz=npz, asset_id='0x'+entry['arena']), textures


def instance_id(layout, kind, index):
    return hashlib.sha256(f'{layout}#{kind}{index}'.encode()).hexdigest()[:16].upper()


# Park teleporter pieces (research b438): the disc vault class pairs each piece id with a channel and a role.
TELEPORTER_CLASS = 'Hash_D01B99210BA7323C'
TELEPORTER_PIECE, TELEPORTER_CHANNEL, TELEPORTER_ROLE = 'Hash_BCFAA75D7A7A5918', 'Hash_DF00F2772AF2D474', 'Hash_77B8CA7AC65C8504'
TELEPORTER_HALF_EXTENTS = [2.0, 2.0, 2.0]  # 8286CFF8: the entry volume, 2.0 (0x82060C50) on each axis


def teleporter_roles(collections):
    """{piece id: (role, channel)} of the teleporter pieces: role 0 start, 1 end (the spawner, role 2, is a piece)."""
    from .environment import key_hash
    result = {}
    for (cls, key), _ in collections.rows.items():
        if cls != key_hash(TELEPORTER_CLASS):
            continue
        fields, _ = collections.resolve(TELEPORTER_CLASS, key)
        value = lambda name: (fields.get(key_hash(name)) or {}).get('data')
        piece, channel, role = value(TELEPORTER_PIECE), value(TELEPORTER_CHANNEL), value(TELEPORTER_ROLE)
        if piece and int(piece, 16) and channel is not None and role is not None and int(role, 16) in (0, 1):
            result[f'{int(piece, 16):016X}'] = (int(role, 16), int(channel, 16))
    return result


def layout_teleporters(sources, stem, stream, parsed, roles):
    """The layout's teleporters in the teleporters.json shape: per channel, the first placed start and end piece
    (8287B918 / 828687C0) become a teleport challenge (8286D6E8: title ID_COMMON_TELEPORT) whose entry volume is a
    box of half extents 2 around the start piece's world centre with its axes, and whose exit is the end piece's
    axes at its world centre (the piece locator's bounds centre, posed by the piece matrix)."""
    from .dynamic_props import locators
    first = {}
    for piece in parsed['static']:
        found = roles.get(piece['piece'])
        if found and found not in first:
            first[found] = piece
    result = []
    for channel in sorted({c for _, c in first}):
        start, end = first.get((0, channel)), first.get((1, channel))
        if start is None or end is None:
            continue
        posed = []
        for piece in (start, end):
            m = matrix(piece['position'], piece['rotation'])
            entry = sources.static.get(piece['piece'])
            bounds = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]
            if entry and entry['arena'] in sources.models:
                found = locators(sources.read(sources.models, entry['arena']))
                if found:
                    bounds = found[0]['bounds']
            local = [(bounds[0][i] + bounds[1][i]) * 0.5 for i in range(3)]
            posed.append((m, (np.array([*local, 1.0]) @ m)[:3].tolist()))
        (start_m, start_centre), (end_m, end_centre) = posed
        exit_matrix = end_m.copy()
        exit_matrix[3, :3] = end_centre
        name = f'skpk_teleporter_entry_vol_{channel}'
        volume_id = instance_id(stem, 't', channel)
        result.append(dict(id=f'skpk:{stem}:{channel}', source='park layout', map=stream[5:] if stream.lower().startswith('dist_') else stream,
            district=stream, layout=stem, volume_name=name, volume_id=volume_id.lower(),
            volume=dict(id=volume_id.lower(), name=name, group='challenge', shape=dict(kind='box', center=start_centre,
                axes=start_m[:3, :3].tolist(), half_extents=TELEPORTER_HALF_EXTENTS, fatness=0.0)),
            title_id='ID_COMMON_TELEPORT', title=None,
            exits=[dict(map=stream[5:] if stream.lower().startswith('dist_') else stream, district=stream,
                        locator=f'{stem}#s{end["index"]}', matrix=exit_matrix.tolist())]))
    return result


def export(sources, worlds, output, report=print, roles=None):
    """Write every layout a world row names (with at least one piece) to `output`; returns {layout: pieces}.
    `roles` (teleporter_roles): the teleporter pieces, written as the layouts' teleporters (`teleporters.json`
    beside the layouts) instead of drawn pieces (retail draws nothing there)."""
    from .dynamic_props import write_placements
    roles = roles or {}
    teleporters = []
    output.mkdir(parents=True, exist_ok=True)
    index, written = {}, {}
    with tempfile.TemporaryDirectory(prefix='skate-park-') as temporary:
        work = Path(temporary)
        templates, movable_templates, textures, missing = {}, {}, {}, set()
        for world, (field_value, stream, *extra) in sorted(worlds.items()):
            colours, lightmap = (extra + [None, None])[:2]
            layout = sources.layout(field_value)
            if layout is None:
                continue  # e.g. `skateparks\debug.park` on the base row: not shipped.
            stem = Path(sources.layouts[layout][1].path.replace('\\', '/')).stem  # the file's own spelling
            index[world] = dict(layout=stem, stream=stream, source=field_value)
            if stem in written:
                continue
            parsed = parse(sources.read(sources.layouts, layout))
            placements = []
            teleporters.extend(layout_teleporters(sources, stem, stream, parsed, roles))
            for piece in parsed['static']:
                if int(piece['piece'], 16) == 0 or piece['piece'] in roles:
                    continue  # retail 82868318 skips id 0; teleporter pieces have no model
                key = piece['piece']
                if key not in templates and key not in missing:
                    try:
                        found = piece_template(sources, key, work)
                    except (ValueError, KeyError, IndexError, struct.error) as error:
                        # One undecodable piece RX2 leaves that piece out, never the whole park (reported per layout).
                        report(f'Park piece {key} ({sources.static.get(key, {}).get("name")}): {error}')
                        found = None
                    if found is None:
                        missing.add(key)
                    else:
                        templates[key], more = found
                        textures.update(more)
                if key not in templates:
                    continue
                recipe_row = sources.static[key]
                placements.append(dict(instance_id=instance_id(stem, 's', piece['index']), template_id=key,
                    matrix=matrix(piece['position'], piece['rotation']).tolist(), name=recipe_row['name'],
                    tint=piece['tint'], caps=recipe_row['caps'], layout_index=piece['index']))
            dynamic = [dict(piece, instance_id=instance_id(stem, 'd', piece['index']),
                            name=sources.dynamic.get(piece['piece'], {}).get('name'),
                            matrix=matrix(piece['position'], piece['rotation']).tolist()) for piece in parsed['dynamic']]
            # The movable pieces: ordinary DMOs of the dynamic recipe (research b435: 82883828 spawns them with the
            # district DMOs' descriptor; template = the mod's arena id; the u32 flag is never read on load), written as
            # the layout's prop package `<layout>.props.skate` / `.props.json` for the engine's prop layer.
            movable = []
            for piece in parsed['dynamic']:
                row = sources.dynamic.get(piece['piece'])
                if int(piece['piece'], 16) == 0 or row is None:
                    missing.add(piece['piece'])
                    continue
                arena = row['arena']
                if arena not in movable_templates and arena not in missing:
                    try:
                        found = piece_template(sources, piece['piece'], work, sources.dynamic)
                    except (ValueError, KeyError, IndexError, struct.error) as error:
                        report(f'Movable park piece {piece["piece"]} ({row["name"]}): {error}')
                        found = None
                    if found is None:
                        missing.add(arena)
                    else:
                        movable_templates[arena], more = found
                        textures.update(more)
                if arena in movable_templates:
                    movable.append(dict(instance_id=instance_id(stem, 'd', piece['index']), template_id=arena,
                        matrix=matrix(piece['position'], piece['rotation']).tolist(), name=row['name'],
                        flag=piece['flag'], layout_index=piece['index']))
            if movable:
                write_placements(stem, stream, movable, movable_templates, textures, output/(stem + '.props.skate'),
                                 layout=layout)
            unresolved = sorted(({p['piece'] for p in parsed['static']} | {p['piece'] for p in parsed['dynamic']}) & missing)
            # The park's projected lightmap, bound as each piece material's lightmap channel (sampled at the projected
            # position, not a UV set; retail_world.wgsl park branch). None: neutral 1, as a world without one.
            lit, layout_templates = None, templates
            if lightmap and lightmap['texture'].lower() in sources.lightmaps:
                name = 'park_lightmap_' + lightmap['texture'].lower()
                if name not in textures:
                    textures[name] = lightmap_texture(sources, lightmap['texture'].lower(), work/(name + '.rgba'))
                lit = dict(lightmap, texture_id=name)
                layout_templates = {key: dict(value, meshes=[dict(mesh, retail_texture_ids=dict(mesh['retail_texture_ids'], lightmap=name))
                                                           for mesh in value['meshes']]) for key, value in templates.items()}
            if placements:
                write_placements(stem, stream, placements, layout_templates, textures, output/(stem + '.skate'),
                                 layout=layout, version=parsed['version'], origin_world_hash=parsed['origin_world_hash'],
                                 dynamic=dynamic, missing_pieces=unresolved, shadow_colours=colours,
                                 projected_lightmap=lit)
            written[stem] = len(placements)
            report(f'Park layout {stem}: {len(placements)} pieces, {len(movable)}/{len(dynamic)} movable, {len(unresolved)} missing')
    index = {w: v for w, v in index.items() if written.get(v['layout'])}
    (output/'worlds.json').write_text(json.dumps(dict(version=1, worlds=index), indent=2), encoding='utf-8')
    (output/'teleporters.json').write_text(json.dumps(dict(version=1, teleporters=teleporters), indent=1), encoding='utf-8')
    report(f'Park teleporters: {len(teleporters)}')
    return written
