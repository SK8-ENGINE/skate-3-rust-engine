"""Challenge visual indicators (the floating ribbon icon over a teleporter; research b532 / b533, USER-ASKS 264).

Two sources:
- `miscload.big data/content/world/models/visual_indicators_sk82.rx2`: an RW4 arena whose object of type 0x00EB0021
  (VISUALINDICATORDATA) holds the ribbon splines. Layout [data]: u32 count, u32 total segments, u32 table offset,
  u32 segment offset; `count` entries of 64 bytes (u64 name hash, u64 spline id, f32x4 bounds min, f32x4 bounds max,
  u32 kind, u32 segments, u32 segment offset from the object, pad); segments of 64 bytes, a 4x4 power-basis cubic
  (rows c3, c2, c1, c0: p(t) = c3 t^3 + c2 t^2 + c1 t + c0, t in [0, 1]).
- the `ribbon_indicator` rows (Offset, the `ribbon_spline` row naming the spline id, the `visualindicators` style row).
Output: assets/private/visual-indicators.json. Details: docs/hails-additions/38-dlc-support.md#teleporter-markers
"""
import json
import struct
from pathlib import Path

from .environment import Collections, key_hash

FORMAT = 'skate3rust.visual-indicators'
VERSION = 1
ARENA = 'data/content/world/models/visual_indicators_sk82.rx2'
SPLINES = 0x00EB0021
OFFSET = 'Hash_4374C0E6F4CB45ED'
SPLINE_REF = 'Hash_8D090C8B5931547A'
STYLE_REF = 'Hash_66FC5F54E2D8B317'
ENABLED = 'Hash_6C09761AE6E69545'
SPLINE_ID = 'Hash_1AFE1B981FB92727'


def arena_objects(data):
    """[(type, offset, size)] of an RW4 arena's object table."""
    if data[:7] != b'\x89RW4xb2':
        raise ValueError('not an RW4 arena')
    count, table = struct.unpack_from('>I', data, 0x20)[0], struct.unpack_from('>I', data, 0x30)[0]
    out = []
    for i in range(count):
        offset, _, size, _, _, kind = struct.unpack_from('>6I', data, table + i * 24)
        out.append((kind, offset, size))
    return out


def parse_splines(data):
    """{spline id '%016X': {kind, name, min, max, segments}} from the arena's VISUALINDICATORDATA object."""
    found = [(o, s) for k, o, s in arena_objects(data) if k == SPLINES]
    if len(found) != 1:
        raise ValueError(f'{len(found)} visual indicator data objects')
    base, size = found[0]
    count, total, table, first = struct.unpack_from('>4I', data, base)
    if table + count * 64 > first or first + total * 64 > size:
        raise ValueError('visual indicator data out of bounds')
    out = {}
    for i in range(count):
        at = base + table + i * 64
        name, spline = struct.unpack_from('>QQ', data, at)
        low, high = struct.unpack_from('>3f', data, at + 16), struct.unpack_from('>3f', data, at + 32)
        kind, segments, offset = struct.unpack_from('>3I', data, at + 48)
        if offset + segments * 64 > size:
            raise ValueError(f'spline {spline:016X} out of bounds')
        rows = [list(struct.unpack_from('>16f', data, base + offset + j * 64)) for j in range(segments)]
        out['%016X' % spline] = dict(name='%016X' % name, kind=kind, min=list(low), max=list(high),
                                     segments=[[r[0:3], r[4:7], r[8:11], r[12:15]] for r in rows])
    return out


def _ref_key(raw):
    """The key hash of a RefSpec value (class u64, key u64, ...)."""
    return struct.unpack_from('>Q', bytes.fromhex(raw), 8)[0]


def indicators(collections):
    """{ribbon_indicator key: {offset, spline, enabled, style}} with inherited fields resolved."""
    cls = key_hash('ribbon_indicator')
    out = {}
    for (row_cls, key), row in collections.rows.items():
        if row_cls != cls:
            continue
        fields, _ = collections.resolve(cls, key)
        spline_row = collections.rows.get((key_hash('ribbon_spline'), _ref_key(fields[key_hash(SPLINE_REF)]['data'])))
        spline = collections.resolve('ribbon_spline', key_hash(spline_row['key']))[0] if spline_row else {}
        style = collections.rows.get((key_hash('visualindicators'), _ref_key(fields[key_hash(STYLE_REF)]['data'])))
        offset = struct.unpack('>3f', bytes.fromhex(fields[key_hash(OFFSET)]['data'])[:12])
        out[row['key']] = dict(offset=list(offset),
                               spline=spline[key_hash(SPLINE_ID)]['data'] if key_hash(SPLINE_ID) in spline else None,
                               enabled=fields[key_hash(ENABLED)]['data'] not in ('', '00'),
                               style=style['key'] if style else None)
    return out


STYLE_FIELDS = dict(width_a='Hash_284E9294277D21DC', width_b='Hash_A6313118DF989363', samples='Hash_4FFE9C7FA608319C',
                    passes='Hash_445E0BFC57EC196B', three_strips='Hash_5D92E494915216BD', fade='Hash_51573D0DA2F211E1',
                    intensity_front='Hash_031DE0F2619E2697', intensity_background='Hash_1643BB6ADCDC41AD')


# The `visualindicators` row names (skatercollections summary report); unnamed rows keep their hashes.
STYLE_NAMES = ('default', 'clintjammie', 'gates', 'lesswiggle', 'non_fade', 'smallarrow', 'smallarrow_nonfade', 'trail')


def style_names():
    """{'Hash_%016X': name} for the known style rows (so mods can name them)."""
    return {'Hash_%016X' % key_hash(n): n for n in STYLE_NAMES}


def styles(collections):
    """{visualindicators key: {width, samples, passes, three_strips, fade, fade_alpha, intensity}}: the fields the draw
    reads (research b534); fade = graph row 1 (x points), fade_alpha = row 2, intensity = shader `i_intensity` (front,
    background; b536)."""
    cls = key_hash('visualindicators')
    out = {}
    for (row_cls, key), row in collections.rows.items():
        if row_cls != cls:
            continue
        fields, _ = collections.resolve(cls, key)
        raw = {n: bytes.fromhex(fields[key_hash(h)]['data']) for n, h in STYLE_FIELDS.items() if key_hash(h) in fields}
        if set(raw) != set(STYLE_FIELDS):
            continue
        graph = struct.unpack('>12f', raw['fade'][:48])
        out[row['key']] = dict(width=[struct.unpack('>f', raw['width_a'][:4])[0], struct.unpack('>f', raw['width_b'][:4])[0]],
                               samples=struct.unpack('>H', raw['samples'][:2])[0],
                               passes=struct.unpack('>H', raw['passes'][:2])[0],
                               three_strips=raw['three_strips'][:1] not in (b'', b'\0'),
                               fade=list(graph[4:8]), fade_alpha=list(graph[8:12]),
                               intensity=[struct.unpack('>f', raw['intensity_front'][:4])[0],
                                          struct.unpack('>f', raw['intensity_background'][:4])[0]])
    return out


PARK_WORLD = 'Hash_0C54AD24155E3220'


def park_maps(collections):
    """The maps whose `world` rows set the park bool (inherited from `default_skateparks`; every row of a stream agrees,
    research b539): the fade end is x10 there (b538)."""
    from .teleports import label_of
    out = set()
    for (row_cls, key), row in collections.rows.items():
        if row_cls != key_hash('world'):
            continue
        fields, _ = collections.resolve('world', key)
        flag, stream = fields.get(key_hash(PARK_WORLD), {}).get('data'), fields.get(key_hash('WorldStream'), {}).get('data')
        if stream and flag and int(flag[:2], 16):
            out.add(label_of(stream))
    return sorted(out)


TEXTURES = 'data/content/textures/vitextures.rx2'
COLOUR_FIELDS = dict(front='Hash_8E38A3CD0186F8DA', background='Hash_FF4DCA217E05B9BC', tint='Hash_E4F3418597AACAA5')


def colours(collections):
    """{ribbon_spline_colour key: {front, background, tint}}: the textures (names without `.Texture`) the two draw
    passes bind and the tint (research b535, `827AEF08`)."""
    cls = key_hash('ribbon_spline_colour')
    out = {}
    for (row_cls, key), row in collections.rows.items():
        if row_cls != cls:
            continue
        fields, _ = collections.resolve(cls, key)
        get = lambda n: fields.get(key_hash(COLOUR_FIELDS[n]), {}).get('data')
        if not get('front') or not get('background'):
            continue
        tint = get('tint')
        out[row['key']] = dict(front=get('front').removesuffix('.Texture'), background=get('background').removesuffix('.Texture'),
                               tint=list(struct.unpack('>4f', bytes.fromhex(tint)[:16])) if tint else [1.0, 1.0, 1.0, 1.0])
    return out


def write_textures(data, folder, sqrt_alpha=()):
    """Every texture of vitextures.rx2 (indicator and volume textures) as PNGs in `folder`; returns their names.
    Textures named in `sqrt_alpha` (the background pass's) store sqrt(alpha): `spline_darkenPS` takes the square
    root of tex.a * intensity * fade (b536), which the engine's material can then do as a plain product."""
    import tempfile
    from tools.vendor.skate3_ui.rw4 import read_textures
    from tools.vendor.skate3_ui.xenos import FetchConstant, decode_rgba, write_png
    folder.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as scratch:
        source = Path(scratch) / 'vitextures.rx2'
        source.write_bytes(data)
        _, textures = read_textures(source)
    names = []
    for texture in textures:
        name = texture.name.removesuffix('.Texture')
        fetch = FetchConstant(**{k: texture.metadata[k] for k in FetchConstant.__dataclass_fields__ if k in texture.metadata})
        rgba = bytearray(decode_rgba(texture.data, fetch))
        if name in sqrt_alpha:
            rgba[3::4] = bytes(round((a / 255) ** 0.5 * 255) for a in rgba[3::4])
        write_png(folder / f'{name}.png', fetch.width, fetch.height, bytes(rgba))
        names.append(name)
    return names


def export(game_root, collections, output, report=print):
    """Writes `output` and returns the indicator keys it holds."""
    from tools.owned_game.big import BigArchive
    archive = BigArchive(Path(game_root) / 'data/big/miscload.big')
    entry = next((e for e in archive.entries if e.path.replace('\\', '/').lower() == ARENA), None)
    if entry is None:
        raise ValueError(ARENA + ' missing')
    splines = parse_splines(archive.read(entry))
    collections = collections if isinstance(collections, Collections) else Collections(collections)
    rows = indicators(collections)
    named = style_names()
    for row in rows.values():
        row['style'] = named.get(row['style'], row['style'])
    found = {named.get(k, k): v for k, v in styles(collections).items()}
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    boot = BigArchive(Path(game_root) / 'data/big/miscboot.big')
    entry = next((e for e in boot.entries if e.path.replace('\\', '/').lower() == TEXTURES), None)
    if entry is None:
        raise ValueError(TEXTURES + ' missing')
    rows_colour = colours(collections)
    textures = write_textures(boot.read(entry), output.parent / 'visual-indicators',
                              {c['background'] for c in rows_colour.values()})
    output.write_text(json.dumps({'format': FORMAT, 'version': VERSION, 'splines': splines, 'indicators': rows,
                                  'styles': found, 'colours': rows_colour, 'textures': 'visual-indicators',
                                  'park_maps': park_maps(collections), 'park_fade_scale': 10.0}), encoding='utf-8')
    missing = sorted(k for k, r in rows.items() if r['spline'] and r['spline'] not in splines)
    report(f'Visual indicators: {len(rows)} rows, {len(splines)} splines, {len(textures)} textures'
           + (f'; splines missing for {", ".join(missing)}' if missing else ''))
    return sorted(rows)
