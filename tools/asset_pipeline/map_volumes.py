"""Export a district's named trigger volumes as the `<Map>.triggers` sidecar.

Retail keeps every named trigger volume as an item of a 0x00EB0019 volume-set
record inside an RW4 simulation arena (ATOC processor 0xAB329A6A) of the
district's cSim_*.xsf streams. TU3 evidence (reference only, no code copied):
fixup 82962538, stream-in 82C9AFD8, trigger group AddVolume 82DD7668 and its
narrow phase 82DD8498 -> 82AD3CD8.

Volume set: +0 magic 0x46DB86E5, +4 count, +8 count, +12 items, +16 strings.
Item (240 bytes): +0 mat4, +64 AABB min, +80 AABB max, +176 u64 link GUID,
+184 u64 instance id (the id challenge scripts use), +216 u32 group word
(the trigger manager 82DD7C58 adds the item to Stairs for 1, Camera for 2 and
Challenge otherwise), +220 index of the item's 0x00EB000A link record, +224
name offset (from the set start).
Link record: +0 index of the box volume, +12 index of the mesh volume.
Box volume (RW collision volume, 0x00080001): +0 3x3 rotation rows (the box's
local axes in world space), +48 centre, +64 type (4 = box), +68 half extents,
+80 fatness. The narrow phase tests the tracked entity's shape against this
oriented box (composed with the item matrix); the AABB only bounds it.

The meshes of these volumes are the surfaceless collision boxes; this module
does not touch collision. Output is JSON (the engine reads it next to the
.skate, like .irradiance), so custom maps can carry hand-written volumes too.
"""
import json
import math
import re
import struct
import tempfile
from pathlib import Path

FORMAT = 'skate3rust-trigger-volumes'
VERSION = 1
SIM_ARENA_PROCESSOR = 0xAB329A6A
VOLUME_SET = 0x00EB0019
LINK_RECORD = 0x00EB000A
RW_VOLUME = 0x00080001
SET_MAGIC = 0x46DB86E5
ITEM_SIZE = 240
BOX = 4
# Item +216 picks the trigger manager group (82DD7C58). Every shipped item
# (11 world, 2,549 missions.big) holds 0 = Challenge, and the recomp trace put
# every AddVolume in Challenge (notes triggers-volumes-re.md §7, §9).
GROUPS = {1: 'stairs', 2: 'camera'}
DEFAULT_GROUP = 'challenge'


def group_of(word):
    return GROUPS.get(word, DEFAULT_GROUP)


def _sections(raw):
    if raw[:7] != b'\x89RW4xb2':
        raise ValueError('Expected an RW4 simulation arena')
    count, table = struct.unpack_from('>I', raw, 32)[0], struct.unpack_from('>I', raw, 48)[0]
    if table + count * 24 > len(raw):
        raise ValueError('Truncated arena section table')
    return [struct.unpack_from('>6I', raw, table + i * 24) for i in range(count)]


def _cstring(raw, offset, limit):
    if not 0 <= offset < limit:
        raise ValueError('Trigger volume name outside its record')
    end = raw.find(b'\0', offset, limit)
    if end < 0:
        raise ValueError('Unterminated trigger volume name')
    return raw[offset:end].decode('latin-1')


def _finite(values, what):
    if not all(math.isfinite(v) for v in values):
        raise ValueError('Non-finite ' + what)
    return [float(v) for v in values]


def short_name(name):
    """`tut_sksc_reset_vol01_0x…:0x…::[…]_HighLOD` -> `tut_sksc_reset_vol01`."""
    return re.split(r'_0x[0-9a-fA-F]', name, maxsplit=1)[0]


def _box(raw, sections, index, item_matrix):
    """World-space oriented box of the volume at section `index`."""
    if not 0 <= index < len(sections) or sections[index][5] != RW_VOLUME:
        raise ValueError('Trigger link does not name a collision volume')
    base, _, size = sections[index][:3]
    if size < 96 or base + size > len(raw):
        raise ValueError('Truncated collision volume')
    kind = struct.unpack_from('>I', raw, base + 64)[0]
    if kind != BOX:
        raise ValueError(f'Trigger volume shape type {kind} is not a box')
    rows = [_finite(struct.unpack_from('>3f', raw, base + 16 * r), 'box rotation') for r in range(3)]
    centre = _finite(struct.unpack_from('>3f', raw, base + 48), 'box centre')
    half = _finite(struct.unpack_from('>3f', raw, base + 68), 'box half extents')
    fatness = _finite(struct.unpack_from('>f', raw, base + 80), 'box fatness')[0]
    if min(half) < 0 or fatness < 0:
        raise ValueError('Negative box extents')
    # Row-vector convention: world = local * M_box * M_item.
    m, t = item_matrix
    axes = [[sum(row[k] * m[k][c] for k in range(3)) for c in range(3)] for row in rows]
    centre = [sum(centre[k] * m[k][c] for k in range(3)) + t[c] for c in range(3)]
    for axis in axes:
        if abs(math.sqrt(sum(v * v for v in axis)) - 1.0) > 1e-3:
            raise ValueError('Trigger box axes are not orthonormal')
    return dict(kind='box', center=centre, axes=axes, half_extents=half, fatness=fatness)


def _matches_aabb(shape, lo, hi, tolerance=0.25):
    # Retail bounds of rotated boxes differ from the box by a few centimetres
    # (wrongway_vol_01: 7 cm); this only proves the link names the right box.
    extent = [sum(abs(shape['axes'][a][c]) * shape['half_extents'][a] for a in range(3)) + shape['fatness']
              for c in range(3)]
    return all(abs(shape['center'][c] - extent[c] - lo[c]) <= tolerance and
               abs(shape['center'][c] + extent[c] - hi[c]) <= tolerance for c in range(3))


def arena_volumes(raw, strict=True):
    """Every named trigger volume in one decoded simulation arena. Not strict: a box that misses its stored bounds is
    kept and flagged `bounds_mismatch` (two challenge hulls, doc 42) instead of failing the arena."""
    sections = _sections(raw)
    result = []
    for base, _, size, _, _, kind in sections:
        if kind != VOLUME_SET:
            continue
        if base + size > len(raw) or size < 20:
            raise ValueError('Truncated volume set')
        magic, count, count2, items, _strings = struct.unpack_from('>5I', raw, base)
        if magic != SET_MAGIC or count != count2 or items + count * ITEM_SIZE > size:
            raise ValueError('Invalid volume set header')
        for index in range(count):
            at = base + items + index * ITEM_SIZE
            mat = _finite(struct.unpack_from('>16f', raw, at), 'volume matrix')
            item_matrix = ([mat[0:3], mat[4:7], mat[8:11]], mat[12:15])
            lo = _finite(struct.unpack_from('>3f', raw, at + 64), 'volume bounds')
            hi = _finite(struct.unpack_from('>3f', raw, at + 80), 'volume bounds')
            if any(a > b for a, b in zip(lo, hi)):
                raise ValueError('Inverted volume bounds')
            link_guid, instance_id = struct.unpack_from('>QQ', raw, at + 176)
            group_word, link, name_offset = struct.unpack_from('>III', raw, at + 216)
            name = _cstring(raw, base + name_offset, base + size)
            if not 0 <= link < len(sections) or sections[link][5] != LINK_RECORD:
                raise ValueError(f'{name}: item link is not a volume link record')
            box_index = struct.unpack_from('>I', raw, sections[link][0])[0]
            shape = _box(raw, sections, box_index, item_matrix)
            mismatch = not _matches_aabb(shape, lo, hi)
            if mismatch and strict:
                raise ValueError(f'{name}: oriented box does not match its bounds')
            result.append(dict(
                id='%016x' % instance_id, name=short_name(name), full_name=name,
                instance_id='%016x' % instance_id, link_guid='%016x' % link_guid,
                group=group_of(group_word), shape=shape, aabb=dict(min=lo, max=hi)))
            if mismatch:
                result[-1]['bounds_mismatch'] = True
    return result


def stream_arenas(stream_directory):
    """(stream file, asset id, decoded arena) for each simulation arena once."""
    from skate3_streams import read_atoc, _decode_section
    stream_directory = Path(stream_directory)
    tables = sorted(stream_directory.glob('*_Sim.xst'))
    if len(tables) != 1:
        raise ValueError('Expected one simulation table in ' + str(stream_directory))
    records = {r.asset_id: r for r in read_atoc(tables[0])}
    seen = set()
    for path in sorted(stream_directory.glob('cSim_*.xsf'), key=lambda p: p.name.casefold()):
        source = path.read_bytes()
        if source[:4] != b'SFIL':
            raise ValueError(f'{path.name} is not a stream file')
        cursor = first = struct.unpack_from('>I', source, 16)[0]
        while cursor + 208 <= len(source):
            asset_id = struct.unpack_from('>Q', source, cursor)[0]
            if asset_id == 0:
                break
            stored, header, stride = struct.unpack_from('>III', source, cursor + 8)
            record = records.get(asset_id)
            if record and record.processor_id == SIM_ARENA_PROCESSOR and asset_id not in seen:
                seen.add(asset_id)
                if stored == record.total_size and header >= 128:
                    data = source[cursor + header:cursor + header + stored]
                else:
                    v = struct.unpack_from('>11I', source, cursor + 128)
                    cpu, at = _decode_section(source, cursor + 208, v[0], v[1], v[2])
                    gpu, _ = _decode_section(source, at, v[8], v[9], v[10])
                    data = cpu + gpu
                if len(data) != record.total_size:
                    raise ValueError(f'{path.name}: arena {asset_id:016x} has the wrong size')
                yield path.name, asset_id, data
            if stride == 0 or stride % first:
                break
            cursor += stride


def district_volumes(stream_directory, strict=True):
    result = []
    for stream, asset_id, arena in stream_arenas(stream_directory):
        for volume in arena_volumes(arena, strict):
            volume.update(stream=stream, arena='%016x' % asset_id)
            result.append(volume)
    ids = [v['id'] for v in result]
    if len(ids) != len(set(ids)):
        raise ValueError('Duplicate trigger volume instance id')
    return result


def document(map_name, volumes):
    return {'format': FORMAT, 'version': VERSION, 'map': map_name,
            'source': 'retail world streams (0x00EB0019 volume sets)', 'volumes': volumes}


def write(path, map_name, volumes):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.tmp')
    temporary.write_text(json.dumps(document(map_name, volumes), indent=1), encoding='utf-8')
    temporary.replace(path)
    return path


def export(stream_directory, output, map_name):
    """Convert one extracted district stream directory into `output`."""
    volumes = district_volumes(stream_directory)
    write(output, map_name, volumes)
    return volumes


def export_archive(archive, output_directory):
    """Stage `<Map>.triggers` from a worldDIST_*.big without a full map conversion."""
    from tools.owned_game.big import BigArchive
    archive = Path(archive)
    district = archive.stem.removeprefix('world')
    big = BigArchive(archive)
    wanted = [e for e in big.entries if Path(e.path).name.startswith('cSim_') or e.path.endswith('_Sim.xst')]
    with tempfile.TemporaryDirectory(prefix='skate-triggers-') as tmp:
        big.extract_entries(wanted, Path(tmp))
        stream = Path(tmp) / 'data/content/world/stream' / district
        label = district.removeprefix('DIST_')
        return export(stream, Path(output_directory) / (label + '.triggers'), label)


if __name__ == '__main__':
    import argparse
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'vendor/university/tools/vanilla_map_extraction/tools'))
    parser = argparse.ArgumentParser(description='Stage trigger-volume sidecars from district archives.')
    parser.add_argument('archives', type=Path, nargs='+', help='worldDIST_*.big')
    parser.add_argument('--output', type=Path, required=True, help='directory for <Map>.triggers')
    arguments = parser.parse_args()
    for archive in arguments.archives:
        rows = export_archive(archive, arguments.output)
        print(archive.stem.removeprefix('worldDIST_'), len(rows), ', '.join(r['name'] for r in rows))
