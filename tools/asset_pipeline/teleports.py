"""Join retail FE destinations to localized names and RX2 location matrices.

Owned content stays in private sidecars. No guessed coordinates or renamed spots.
LocationManager 828A3D50 loads data/content/global_locators; EB0009 records
contain a 64-byte matrix and a section-relative name pointer at +104.
"""
import hashlib
import json
import struct
import tempfile
from pathlib import Path

from tools.owned_game.big import BigArchive
from .dynamic_props import sections, matrix
from .environment import Collections, key_hash
from .vlt import vault


def label_of(district):
    """The map label of a district stream (as asset_pipeline.dlc.map_jobs and the disc jobs name maps); the prefix
    in any case (Zen's stream is `Dist_Zen`)."""
    return district[5:] if district.lower().startswith('dist_') else district


def cstring(raw, offset, limit=None):
    limit = len(raw) if limit is None else limit
    if not 0 <= offset < limit <= len(raw):
        raise ValueError('Invalid location string offset')
    return raw[offset:raw.index(b'\0', offset, limit)]


def language_table(raw):
    if len(raw) < 36 or struct.unpack_from('<I', raw)[0] != 0x39000:
        raise ValueError('Unsupported retail language table')
    count, start, strings = struct.unpack_from('<3I', raw, 8)
    start += 8
    strings += 8
    if start + count * 8 != strings or strings > len(raw):
        raise ValueError('Invalid retail language index')
    result = {}
    for index in range(count):
        key, offset = struct.unpack_from('<2I', raw, start + index * 8)
        if key in result:
            raise ValueError('Duplicate language key')
        result[key] = cstring(raw, strings + offset)
    return result


def location_records(raw):
    result = []
    for base, _, size, _, _, kind in sections(raw):
        if kind != 0xEB0009:
            continue
        count, _, _, start, end = struct.unpack_from('>5I', raw, base)
        if base + size > len(raw) or start != 32 or end != start + count * 128 or end > size:
            raise ValueError('Invalid location record table')
        for index in range(count):
            at = base + start + index * 128
            pointer = struct.unpack_from('>I', raw, at + 104)[0]
            if not end <= pointer < size:
                raise ValueError('Location name overlaps records')
            result.append({'locator': cstring(raw, base + pointer, base + size).decode('utf-8'),
                           'matrix': matrix(raw, at).tolist(), 'source_offset': at})
    return result


_PARK_SOURCES = {}


def park_start(game_root, field_value, pack_bigs=()):
    """The start matrix of the `.park` layout a world row names (disc miscload.big, then the player's packs), or
    None."""
    from .park_layout import Sources, parse
    root = (str(game_root), tuple(str(b) for b in pack_bigs))
    if root not in _PARK_SOURCES:
        _PARK_SOURCES[root] = Sources([BigArchive(Path(game_root) / 'data/big/miscload.big')]
                                      + [BigArchive(b) for b in pack_bigs])
    sources = _PARK_SOURCES[root]
    layout = sources.layout(field_value)
    return parse(sources.read(sources.layouts, layout))['start'] if layout else None


def convert(game_root, assets, converted, dlc_vaults=()):
    """`dlc_vaults` (asset_pipeline.dlc.vaults): the player's packs' rows join the disc's, each resolving its text
    against its own vault's .bin, with the packs' locators, language and park layouts."""
    game_root, assets = Path(game_root), Path(assets)
    from .dlc import language as pack_language, merged, stream_districts
    dlc_vaults = list(dlc_vaults)
    binaries = {v['vault']: v['binary'] for v in dlc_vaults}
    pack_bigs = list(dict.fromkeys(v['big'] for v in dlc_vaults))
    pack_text, pack_streams = {}, set()
    for big in pack_bigs:
        archive = BigArchive(big)
        for label, value in pack_language(archive).items():
            pack_text.setdefault(label, value)
        pack_streams.update(d.casefold() for d in stream_districts(
            [e.path.replace('\\', '/').lower() for e in archive.entries]))
    archive = BigArchive(game_root / 'data/big/miscboot.big')
    tables = {}
    for language in ('english', 'labels'):
        entry = next(e for e in archive.entries if f'/{language}/' in e.path and '_Global_' in e.path)
        tables[language] = language_table(archive.read(entry))
    labels = {value.strip().decode('ascii'): key for key, value in tables['labels'].items()}
    database = BigArchive(game_root / 'data/big/db.big')
    with tempfile.TemporaryDirectory(prefix='skate-teleports-') as tmp:
        root = Path(tmp)
        entries = [e for e in database.entries if e.path in
                   ('data/db/skatercollections.bin', 'data/db/skatercollections.vlt')]
        database.extract_entries(entries, root)
        _, binary, _ = vault(root / 'data/db/skatercollections')
    locators = {}
    for path in sorted((game_root / 'data/content/global_locators').rglob('*.rx2')):
        raw = path.read_bytes()
        stream = path.parent.name
        for record in location_records(raw):
            record.update(map=label_of(stream), source=str(path.relative_to(game_root)),
                          source_sha256=hashlib.sha256(raw).hexdigest())
            locators.setdefault((stream.casefold(), record['locator']), []).append(record)
    for v in dlc_vaults:
        source = 'pack ' + Path(v['big']).name
        for key, matrices in v['locators'].items():
            locators.setdefault(key, []).extend(dict(matrix=m, source=source) for m in matrices)
    for v in dlc_vaults:
        # A DLC stream's own _Sim.loc only for names the global locators lack (map_starts.all_locators).
        source = 'pack ' + Path(v['big']).name
        for key, matrices in v.get('stream_locators', {}).items():
            if key not in locators:
                locators[key] = [dict(matrix=m, source=source + ' stream') for m in matrices]
    collections = Collections(merged(converted, dlc_vaults))

    def text_of(label):
        """The menu text of a label, disc first, then the packs' (None: a template placeholder)."""
        if label in labels:
            return tables['english'][labels[label]]
        return pack_text[label].encode('latin-1') if pack_text.get(label) else None

    def stream_exists(stream):
        return (game_root / 'data/content' / ('world' + stream + '.big')).exists() or stream.casefold() in pack_streams
    world_keys = {key: row['key'] for (cls, key), row in collections.rows.items() if cls == key_hash('world')}
    destinations = []
    # Own location artwork distinguishes selectable FE destinations from base
    # templates and startup checkpoints. A skate.Park layout row (`megapark_full`, research b424) has no artwork of its
    # own but is a destination: its world places the park's pieces; it is named by its world row's text.
    for (cls, key), row in collections.rows.items():
        if cls != key_hash('fe_locations'):
            continue
        fields, _ = collections.resolve(cls, key)
        world_field = fields.get(key_hash('World'))
        world_fields = collections.resolve('world', int(world_field['data'][:16], 16))[0] if world_field else {}
        park = key_hash('Hash_79B248E8469DC5DA') in world_fields
        if not park and not any(key_hash(k) == key_hash('Hash_786A2D24F475342B') for k in row['fields']):
            continue
        label = fields[key_hash('Hash_174B301910A902C9')]['data']
        if park and not any(key_hash(k) == key_hash('Hash_786A2D24F475342B') for k in row['fields']):
            label = (world_fields.get(key_hash('Hash_095E1D9778F0C6F4')) or {}).get('data')
        text = text_of(label)
        if text is None:
            continue  # Template placeholders have no shipped localized name.
        # 0xAB is the retail font's skate wordmark glyph. The portable UI has
        # no such font glyph; spell out the same brand instead of showing «.
        name = text.replace(b'\xab', b'skate.').decode('ascii').strip()
        own_binary = binaries.get(row.get('vault'), binary)
        locator = cstring(own_binary, int(fields[key_hash('location')]['data'], 16)).decode('utf-8')
        world_key = int(fields[key_hash('World')]['data'][:16], 16)
        world, _ = collections.resolve('world', world_key)
        stream = world[key_hash('WorldStream')]['data']
        if not stream_exists(stream):
            continue
        matches = locators.get((stream.casefold(), locator), [])
        if not matches:
            # Retail resolves the name in one global locator table, whatever the district (research b436: locator
            # system 82910DC0 over all of global_locators): the Stadium's layout rows name DownTown park's
            # `Z_SP_DT_Start` and use it as is. Same-district first keeps every other destination as before.
            matches = [{k: m[k] for k in ('matrix', 'source', 'source_sha256') if k in m}
                       for (_, name), found in locators.items() if name == locator for m in found]
            if len({json.dumps(m['matrix']) for m in matches}) > 1:
                matches = []
            if not matches and park:
                # A name found nowhere (`vert_skp5_01_challengelocator_01`): 82910DC0 teleports to identity at
                # (0, 0.5, -5 x (player + 1)); player 1 here.
                item_matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.5, -5.0, 1.0]]
                matches = [dict(matrix=item_matrix, source='retail missing-locator fallback (82910DC0)')]
        layout_field = world_fields.get(key_hash('Hash_79B248E8469DC5DA'))
        artwork = any(key_hash(k) == key_hash('Hash_786A2D24F475342B') for k in row['fields'])
        if park and layout_field and layout_field.get('data') and not (artwork and matches):
            # A skate.Park world places the skater at its layout's start (the .park editor tail, park_layout.parse),
            # not at the location's locator: the user's retail video (2026-10-21) starts Stadium - Full on top of the
            # big ramp. A location with its own artwork and locator (startpark_progression) keeps it (ours: retail's
            # rule for those rows is not traced); a layout without a start keeps the locator rule above.
            start = park_start(game_root, layout_field['data'], pack_bigs)
            if start is not None:
                matches = [dict(matrix=start, source='park layout start ' + layout_field['data'])]
        unique = {json.dumps(m['matrix']) for m in matches}
        if len(unique) > 1:
            raise ValueError('Ambiguous authored teleport: ' + locator)
        item = dict(id=row['key'], name=name, map=label_of(stream), world=world_keys.get(world_key),
                    label_key=label, locator=locator, matrix=None)
        if matches:
            item.update(matches[0])
        else:
            item['unavailable_reason'] = 'Authored destination locator was not found in the extracted content'
        destinations.append(item)
    destinations.sort(key=lambda d: (d['map'].casefold(), d['name'].casefold()))
    output = assets / 'private/teleports.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({'version': 1, 'destinations': destinations}, indent=2), encoding='utf-8')
    return destinations
