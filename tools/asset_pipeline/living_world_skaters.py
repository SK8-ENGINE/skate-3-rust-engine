"""Living world, NPC skater half: export the recorded skater lines and the
character / profile tables from the user's disc.

Outputs (under ``<assets>/private/living_world/``):

``skater_paths/<district>.bin``
    One pack per city district (DownTown, University, Industrial; the park
    districts have no lines). Our container (little-endian header) around the
    retail AIPATHDATA objects (RenderWare type 0x00EB0014), stored verbatim,
    one per ``cSim_*`` tile stream asset:

    ====  ========================================================
    +0    8 B  ``LWSKPTH\\0``
    +8    u32  version (1)
    +12   u32  tile count
    +16   tile table, 48 B each: u64 asset id, u32 blob offset (from
          the file start), u32 blob length, 32 B tile stream name
          (NUL padded, e.g. ``cSim_-150_-150_high``)
    then  the blobs, each 16-byte aligned
    ====  ========================================================

    Why raw blobs: one decoder (``skate-data::aipath``) reads the disc and the
    export alike, nothing is lost or re-quantised, the tile split matches how
    retail streams the lines (a line crossing tiles is stored in each, keyed by
    its 16-byte id), and a mod can author extra lines in the same retail layout
    (DumbadsSkate3ModdingTools writes it) or patch them by path id.

``skater_paths/index.json``
    Per district: tiles (name, asset id, path copies) and every unique path by
    its id (32 hex digits): tiles, node count, flags, allowed-skater mask,
    start node, bbox. The id is the stable key a content overlay patches.

``skater_profiles.json``
    The resolved vault rows (inheritance applied) of ``ai_skater``,
    ``ai_skater_profiles`` and ``characters_marquee``, keyed by record name,
    with the layout bytes +8 / +9 and the free-roam pool rule spelt out.

Entry point: :func:`export` (``ctx`` gives the disc root, the output and a
scratch folder). Values come from the user's disc at setup time only.
"""
from __future__ import annotations

import hashlib
import json
import struct
import sys
from pathlib import Path

AIPATHDATA = 0x00EB0014
REGION_PROCESSOR = 0xAB329A6A  # world-painter arenas; the AIPATH objects live in them
PATH_STRIDE, NODE_STRIDE, BRANCH_GROUP_STRIDE, BRANCH_STRIDE = 0x60, 44, 12, 24
PACK_MAGIC, PACK_VERSION, PACK_TILE_STRIDE, PACK_NAME_LEN = b'LWSKPTH\0', 1, 48, 32
CITY_TAGS = {b'dwtn': 'DownTown', b'univ': 'University', b'indu': 'Industrial'}
VERSION = 1

DATABASE_FILES = ('skaterschema.bin', 'skaterschema.vlt', 'skatercollections.bin', 'skatercollections.vlt',
                  'skaterschema_summaryreport.txt', 'skatercollections_summaryreport.txt')
PROFILE_CLASSES = ('ai_skater', 'ai_skater_profiles', 'characters_marquee')

# characters_marquee fields (skaterschema.vlt). Layout (size 20): +0 recipe, +4 display name id, +8, +9,
# +12 voice / cast id, +16, +17; the rest are optional attributes. Names from the notes
# (.claude/notes/npc-skaters-re.md section 2 and "Answers (2026-10-04)").
MARQUEE_FIELDS = {
    'recipe': 'Hash_6C9F05D8DBE7A492',             # marquee recipe (model / outfit)
    'name_id': 'Hash_174B301910A902C9',            # display name string id
    'layout_8': 'Hash_95E4332C22316FF9',           # layout byte +8 (with +9: front-end Call Skater pro list, sub_8260FA08)
    'layout_9': 'Hash_9A19F602E62EC9E0',           # layout byte +9: free-roam AI pool (sub_82461550)
    'voice': 'Hash_117BF4CD5C49C4C5',              # Sk8::Audio::eSk8Characters (speech cast)
    'layout_16': 'Hash_A43E91B7F8380244',
    'layout_17': 'Hash_49283EF74A020808',          # 1 for ambient + community records
    'aiprofile': 'Hash_CDC34D0245E77199',          # RefSpec into ai_skater_profiles
    'community': 'Hash_31A41CDD1EE6C5AC',          # read by sub_82C2C168
    'teammate': 'Hash_46437782A3CDBAEA',           # read by sub_82C2C1F8 / sub_82458228
    'teammate_index': 'Hash_337CF781F31C8877',     # save slot index (sub_82458280)
    'linked': 'Hash_4FAFF7D80EAF9779',             # linked record (sub_82461690)
}
PROFILE_PRO_INDEX = 'Hash_A390BE5FC0DDA71C'        # profile +140 pro index (sub_8245FEE0); bit of m_AllowedSkaters


# ---------------------------------------------------------------- AIPATH objects

def arena_objects(arena: bytes, object_type: int = AIPATHDATA) -> list[bytes]:
    """Every object of one type in an RW4 arena (directory count at +0x20, offset at +0x30,
    24-byte entries: offset, ?, size, ?, ?, type)."""
    if len(arena) < 0x34:
        return []
    count, directory = struct.unpack_from('>I', arena, 0x20)[0], struct.unpack_from('>I', arena, 0x30)[0]
    if directory + 24 * count > len(arena):
        raise ValueError('arena directory runs past the end')
    out = []
    for i in range(count):
        offset, _, size, _, _, kind = struct.unpack_from('>6I', arena, directory + 24 * i)
        if kind == object_type:
            if offset + size > len(arena):
                raise ValueError(f'AIPATH object {i} runs past the arena end')
            out.append(arena[offset:offset + size])
    return out


def paths_in(blob: bytes) -> list[dict]:
    """Header summary of every path in an AIPATHDATA blob (the full decoder is skate-data::aipath;
    this checks the same layout for the setup report)."""
    count, array = struct.unpack_from('>II', blob, 0)
    if array < 8 or array + count * PATH_STRIDE > len(blob):
        raise ValueError(f'AIPATH header: {count} paths at {array:#x} in {len(blob)} bytes')
    out = []
    for i in range(count):
        h = array + i * PATH_STRIDE
        bmin, bmax = struct.unpack_from('>3f', blob, h), struct.unpack_from('>3f', blob, h + 0x10)
        pid = blob[h + 0x20:h + 0x30]
        nodes_at, nodes, _ext, groups_at, groups, flags = struct.unpack_from('>6I', blob, h + 0x30)
        mask, = struct.unpack_from('>Q', blob, h + 0x48)
        skill, = struct.unpack_from('>i', blob, h + 0x50)
        first = h + nodes_at
        if first + nodes * NODE_STRIDE > len(blob):
            raise ValueError(f'AIPATH path {i}: nodes run past the end')
        start = struct.unpack_from('>3f', blob, first) if nodes else None
        length = 0.0
        prev = start
        for k in range(1, nodes):
            p = struct.unpack_from('>3f', blob, first + k * NODE_STRIDE)
            length += sum((a - b) ** 2 for a, b in zip(p, prev)) ** .5
            prev = p
        branches = []
        for g in range(groups):
            go = h + groups_at + g * BRANCH_GROUP_STRIDE
            rel, n, node = struct.unpack_from('>III', blob, go)
            for b in range(n):
                bo = go + rel + b * BRANCH_STRIDE
                if bo + BRANCH_STRIDE > len(blob):
                    raise ValueError(f'AIPATH path {i}: branch runs past the end')
                branches.append((node, blob[bo:bo + 16].hex(), struct.unpack_from('>I', blob, bo + 16)[0]))
        out.append({'id': pid.hex(), 'tag': pid[1:5], 'nodes': nodes, 'flags': flags, 'allowed_skaters': mask,
                    'skill_level': skill, 'bbox': [list(bmin), list(bmax)], 'start': list(start) if start else None,
                    'length': length, 'branches': branches})
    return out


def write_pack(tiles: list[tuple[int, str, bytes]]) -> bytes:
    """The pack bytes; identical to skate_data::aipath::write_pack."""
    def align(n):
        return (n + 15) & ~15
    head = bytearray(PACK_MAGIC + struct.pack('<II', PACK_VERSION, len(tiles)))
    cursor = align(16 + len(tiles) * PACK_TILE_STRIDE)
    for asset_id, name, blob in tiles:
        encoded = name.encode('ascii')[:PACK_NAME_LEN - 1]
        head += struct.pack('<QII', asset_id, cursor, len(blob)) + encoded.ljust(PACK_NAME_LEN, b'\0')
        cursor = align(cursor + len(blob))
    out = bytearray(head)
    for _, _, blob in tiles:
        out += b'\0' * (align(len(out)) - len(out))
        out += blob
    return bytes(out)


def read_pack(data: bytes) -> list[tuple[int, str, bytes]]:
    if data[:8] != PACK_MAGIC:
        raise ValueError('not a skater path pack')
    version, count = struct.unpack_from('<II', data, 8)
    if version != PACK_VERSION:
        raise ValueError(f'unsupported skater path pack version {version}')
    out = []
    for t in range(count):
        asset_id, offset, length = struct.unpack_from('<QII', data, 16 + t * PACK_TILE_STRIDE)
        name = data[32 + t * PACK_TILE_STRIDE:32 + t * PACK_TILE_STRIDE + PACK_NAME_LEN].split(b'\0')[0].decode('ascii')
        out.append((asset_id, name, data[offset:offset + length]))
    return out


def district_tiles(game_root: Path, work: Path):
    """Yield (district, tile stream name, asset id, AIPATHDATA blob) for every city tile.
    Asset copies stored in several cells are read once (same rule as the region export)."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/university/tools/vanilla_map_extraction/tools'))
    from skate3_streams import read_atoc, read_sfil
    from tools.owned_game.big import BigArchive
    for archive in sorted((game_root/'data/content').glob('worldDIST_*.big')):
        district = archive.stem.removeprefix('worldDIST_')
        big = BigArchive(archive)
        index = next((e for e in big.entries if e.path.endswith('_Sim.xst')), None)
        if index is None:
            continue
        folder = work/'skater_paths'/district
        folder.mkdir(parents=True, exist_ok=True)
        (folder/'index.xst').write_bytes(big.read(index))
        records = read_atoc(folder/'index.xst')
        by_id = {r.asset_id: r for r in records}
        seen = set()
        for entry in sorted(big.entries, key=lambda e: e.path):
            name = Path(entry.path).name
            if not (name.startswith('cSim_') and name.endswith('.xsf')):
                continue
            stream = folder/name
            stream.write_bytes(big.read(entry))
            for asset in read_sfil(stream, records, require_all_records=False, record_index=by_id):
                if asset.record.processor_id != REGION_PROCESSOR or asset.record.asset_id in seen:
                    continue
                seen.add(asset.record.asset_id)
                for blob in arena_objects(asset.data):
                    yield district, Path(name).stem, asset.record.asset_id, blob
            stream.unlink()


def export_paths(tiles, output: Path) -> tuple[dict, list[str]]:
    """Write the packs + index from (district, tile, asset id, blob) tuples; return (index, warnings)."""
    by_district: dict[str, list[tuple[int, str, bytes]]] = {}
    for district, tile, asset_id, blob in tiles:
        by_district.setdefault(district, []).append((asset_id, tile, blob))
    output.mkdir(parents=True, exist_ok=True)
    index, warnings = {'version': VERSION, 'districts': {}}, []
    all_ids: dict[str, int] = {}
    pending_branches = []
    for district, entries in sorted(by_district.items()):
        entries.sort(key=lambda e: (e[1], e[0]))
        tiles_out, paths_out = [], {}
        for asset_id, tile, blob in entries:
            summary = paths_in(blob)
            tiles_out.append({'name': tile, 'asset_id': f'{asset_id:016X}', 'paths': len(summary)})
            for path in summary:
                if path['tag'] not in CITY_TAGS:
                    warnings.append(f'{district}/{tile}: path {path["id"]} has tag {path["tag"]!r}')
                known = paths_out.get(path['id'])
                if known:
                    if (known['nodes'], known['flags'], known['allowed_skaters']) != (
                            path['nodes'], path['flags'], f'{path["allowed_skaters"]:016X}'):
                        warnings.append(f'{district}: copies of path {path["id"]} differ (first copy kept)')
                    known['tiles'].append(tile)
                    continue
                all_ids[path['id']] = path['nodes']
                pending_branches.extend((district, path['id'], b) for b in path['branches'])
                paths_out[path['id']] = {
                    'tiles': [tile], 'nodes': path['nodes'], 'flags': path['flags'],
                    'allowed_skaters': f'{path["allowed_skaters"]:016X}', 'skill_level': path['skill_level'],
                    'start': path['start'], 'bbox': path['bbox'], 'length': round(path['length'], 3),
                    'branches': len(path['branches'])}
        (output/f'{district}.bin').write_bytes(write_pack(entries))
        index['districts'][district] = {
            'file': f'{district}.bin', 'tiles': tiles_out, 'path_copies': sum(t['paths'] for t in tiles_out),
            'paths': paths_out, 'nodes': sum(p['nodes'] for p in paths_out.values())}
    for district, pid, (node, target, target_node) in pending_branches:
        if target not in all_ids:
            warnings.append(f'{district}: path {pid} node {node} branches to unknown path {target}')
        elif target_node >= all_ids[target]:
            warnings.append(f'{district}: path {pid} branches past the end of {target}')
    (output/'index.json').write_text(json.dumps(index, indent=1), encoding='utf-8')
    return index, warnings


# ---------------------------------------------------------------- vault tables

def convert_database(game_root: Path, work: Path) -> dict:
    """skatercollections converted with the names the disc ships (same inputs as the
    customiser's database conversion, customisation_native.py)."""
    from tools.owned_game.big import BigArchive
    from .vlt import convert
    archive = BigArchive(game_root/'data/big/db.big')
    staging = work/'database'
    staging.mkdir(parents=True, exist_ok=True)
    found = set()
    for entry in archive.entries:
        name = Path(entry.path).name.lower()
        if entry.path.lower().startswith('data/db/') and name in DATABASE_FILES:
            (staging/name).write_bytes(archive.read(entry))
            found.add(name)
    missing = {'skaterschema.bin', 'skaterschema.vlt', 'skatercollections.bin', 'skatercollections.vlt'} - found
    if missing:
        raise ValueError(f'db.big lacks {sorted(missing)}')
    names = Path(__file__).with_name('names.txt').read_text(encoding='utf-8').splitlines()
    for report in ('skaterschema_summaryreport.txt', 'skatercollections_summaryreport.txt'):
        if (staging/report).exists():
            for line in (staging/report).read_text(encoding='latin-1').splitlines():
                names.extend(line.replace('.class', '').replace('.xml', '').split('/'))
    return convert(staging/'skaterschema', staging/'skatercollections', names)


def _field_name(fields: dict, name: str) -> str | None:
    """A field by its plain name or its Hash_ form (the conversion leaves unnamed fields hashed)."""
    from .vlt import hash64
    if name.startswith('Hash_'):
        if name in fields:
            return name
        return next((k for k in fields if not k.startswith('Hash_') and f'Hash_{hash64(k):016X}' == name), None)
    if name in fields:
        return name
    hashed = f'Hash_{hash64(name):016X}'
    return hashed if hashed in fields else None


def decode_value(field: dict, refs: dict[int, tuple[str, dict[int, str]]]):
    """A converted vault field as JSON: numbers, bools, text, resolved RefSpecs, arrays."""
    kind, data = field.get('type', ''), field.get('data')
    if 'array' in field:
        items = field['array'].get('items', [])
        if kind == 'Sk8::AI::ProfileTrickEntry':
            return [{'trick': int(i[:8], 16), 'weight': struct.unpack('>f', bytes.fromhex(i[8:16]))[0]} for i in items]
        return [decode_value({'type': kind, 'data': i}, refs) for i in items]
    if data is None:
        return None
    if kind == 'EA::Reflection::Text':
        return data
    raw = bytes.fromhex(data) if isinstance(data, str) else b''
    if kind == 'EA::Reflection::Float' and len(raw) == 4:
        return round(struct.unpack('>f', raw)[0], 7)
    if kind == 'EA::Reflection::Bool' and raw:
        return raw[0] != 0
    if kind in ('EA::Reflection::UInt8', 'EA::Reflection::Int8') and raw:
        return raw[0] if kind.endswith('UInt8') else struct.unpack('>b', raw[:1])[0]
    if kind == 'EA::Reflection::Int32' and len(raw) == 4:
        return struct.unpack('>i', raw)[0]
    if kind == 'EA::Reflection::UInt32' and len(raw) == 4:
        return struct.unpack('>I', raw)[0]
    if kind == 'Attrib::RefSpec' and len(raw) >= 16:
        class_key, record_key = struct.unpack('>QQ', raw[:16])
        if class_key in refs:
            name, keys = refs[class_key]
            return {'class': name, 'key': keys.get(record_key, f'{record_key:016X}')}
        return {'class': f'{class_key:016X}', 'key': f'{record_key:016X}'}
    # 32-bit enums (Sk8::Audio::eSk8Characters, Sk8::AIPath::EAISkater, ...): their value.
    if len(raw) == 4 and kind.rsplit('::', 1)[-1][:1] in ('e', 'E'):
        return struct.unpack('>i', raw)[0]
    return {'type': kind, 'hex': data}


def resolve_class(rows: list[dict], cls: str, refs) -> dict[str, dict]:
    """Rows of one class with inheritance applied (parent fields first, child overrides)."""
    by_key = {r['key']: r for r in rows if r['class'] == cls}
    out = {}
    for key, row in by_key.items():
        chain, node = [], row
        while node:
            if node['key'] in [c['key'] for c in chain]:
                raise ValueError(f'cyclic {cls} inheritance at {key}')
            chain.append(node)
            node = by_key.get(node.get('parent', ''))
        merged = {}
        for node in reversed(chain):
            merged.update(node['fields'])
        out[key] = {'parent': row.get('parent', ''), 'raw': merged,
                    'fields': {name: decode_value(f, refs) for name, f in sorted(merged.items())}}
    return out


def profiles(collections: list[dict]) -> tuple[dict, list[str]]:
    """skater_profiles.json content: (document, warnings)."""
    from .vlt import hash64
    warnings = []
    refs = {hash64(cls): (cls, {hash64(r['key']): r['key'] for r in collections if r['class'] == cls})
            for cls in {r['class'] for r in collections}}
    tables = {cls: resolve_class(collections, cls, refs) for cls in PROFILE_CLASSES}
    for cls in PROFILE_CLASSES:
        if not tables[cls]:
            warnings.append(f'vault class {cls} has no records')
    ai_profiles = tables['ai_skater_profiles']
    characters = {}
    for key, row in sorted(tables['characters_marquee'].items()):
        fields, raw = row['fields'], row['raw']
        def get(alias, default=None):
            name = _field_name(raw, MARQUEE_FIELDS[alias])
            return fields[name] if name is not None else default
        profile = get('aiprofile')
        profile_key = profile.get('key') if isinstance(profile, dict) else None
        if profile_key is not None and profile_key not in ai_profiles:
            warnings.append(f'characters_marquee/{key}: aiprofile {profile_key} is not an ai_skater_profiles record')
        pro_index = None
        if profile_key in ai_profiles:
            name = _field_name(ai_profiles[profile_key]['raw'], PROFILE_PRO_INDEX)
            pro_index = ai_profiles[profile_key]['fields'][name] if name else None
        teammate = bool(get('teammate', False))
        in_pool = bool(get('layout_9', False))
        characters[key] = {
            'parent': row['parent'],
            'recipe': get('recipe', '') or None,
            'name_id': get('name_id', '') or None,
            'voice': get('voice'),
            'aiprofile': profile_key,
            'pro_index': pro_index,
            'layout': {'+8': bool(get('layout_8', False)), '+9': in_pool,
                       '+16': bool(get('layout_16', False)), '+17': bool(get('layout_17', False))},
            'community': bool(get('community', False)),
            'teammate': teammate,
            'teammate_index': get('teammate_index') if teammate else None,
            'linked': get('linked'),
            # sub_82461550: byte +9 set; teammates also need their save slot filled (recruited),
            # community records only offline. Group records (no recipe) never spawn.
            'free_roam_pool': in_pool and bool(get('recipe', '')),
            'needs': ('recruited_save_slot' if teammate else 'offline' if get('community', False) else None),
            'fields': fields,
        }
    doc = {
        'version': VERSION,
        'ai_skater': {k: {'parent': v['parent'], 'fields': v['fields']} for k, v in sorted(tables['ai_skater'].items())},
        'ai_skater_profiles': {k: {'parent': v['parent'], 'fields': v['fields']} for k, v in sorted(ai_profiles.items())},
        'characters': characters,
        'free_roam_pool': sorted(k for k, c in characters.items() if c['free_roam_pool']),
    }
    return doc, warnings


# ---------------------------------------------------------------- entry point

def _get(ctx, name, default=None):
    if isinstance(ctx, dict):
        return ctx.get(name, default)
    return getattr(ctx, name, default)


def export(ctx) -> dict:
    """Export the NPC skater half of the living world.

    ``ctx`` (a dict or an object with these attributes):
      ``game_root``  extracted disc root (the folder holding ``default.xex`` and ``data/``)
      ``private``    the stage's ``assets/private`` folder; or ``stage`` (``<stage>/assets/private``)
      ``work``       scratch folder (deleted by the caller)
      ``report``     optional progress callback ``report(text)``
      ``collections`` optional already-converted vault rows (list) to skip the conversion

    Writes ``<private>/living_world/skater_paths/*`` and ``skater_profiles.json``; returns a
    report dict (counts, warnings)."""
    game_root = Path(_get(ctx, 'game_root'))
    private = _get(ctx, 'private')
    private = Path(private) if private is not None else Path(_get(ctx, 'stage'))/'assets/private'
    work = Path(_get(ctx, 'work'))/'living_world_skaters'
    report = _get(ctx, 'report') or (lambda text: None)
    output = private/'living_world'
    work.mkdir(parents=True, exist_ok=True)

    report('Exporting NPC skater lines')
    index, warnings = export_paths(district_tiles(game_root, work), output/'skater_paths')

    report('Exporting NPC skater characters and profiles')
    collections = _get(ctx, 'collections')
    if collections is None:
        collections = convert_database(game_root, work)['collections']
    doc, profile_warnings = profiles(collections)
    warnings += profile_warnings
    path_ids = {pid for d in index['districts'].values() for pid in d['paths']}
    (output/'skater_profiles.json').write_text(json.dumps(doc, indent=1), encoding='utf-8')

    districts = {name: {'tiles': len(d['tiles']), 'path_copies': d['path_copies'], 'paths': len(d['paths']),
                        'nodes': d['nodes']} for name, d in index['districts'].items()}
    return {
        'version': VERSION,
        'districts': districts,
        'path_copies': sum(d['path_copies'] for d in districts.values()),
        'paths': len(path_ids),
        'nodes': sum(d['nodes'] for d in districts.values()),
        'characters': len(doc['characters']),
        'profiles': len(doc['ai_skater_profiles']),
        'free_roam_pool': len(doc['free_roam_pool']),
        'outputs': sorted(str(p.relative_to(private)).replace('\\', '/') for p in output.rglob('*') if p.is_file()),
        'sha256': {str(p.relative_to(private)).replace('\\', '/'): hashlib.sha256(p.read_bytes()).hexdigest()
                   for p in sorted(output.rglob('*')) if p.is_file()},
        'warnings': warnings,
    }
