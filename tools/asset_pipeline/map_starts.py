"""Authored default spawn (position + heading) per district from retail data.

The map spawn is every map's startup position and the menu's "default landing
zone". Retail data names it directly for skate parks; other districts use one
retail FE location. Positions/headings come from the district's own
global_locators EB0009 records (see teleports.py), never from geometry.

Priority per district:
1. `world` row start locator (field Hash_3735C5C12E8E7AE1; the skate parks),
2. DEFAULT_LOCATIONS: an fe_locations row chosen for districts with several
   (cities, Maloof) — project choices, documented in docs/hails-additions,
3. the district's only fe_locations row (BlackBoxPark, SkateSchool).
Each district tries its candidates in that order; a locator still at the identity (never placed) is skipped.
Districts without a match keep the geometric rule in map_writer.
"""
import json
import math
import tempfile
from pathlib import Path

from tools.owned_game.big import BigArchive
from .environment import Collections, key_hash
from .teleports import cstring, location_records
from .vlt import vault

START_LOCATOR = 'Hash_3735C5C12E8E7AE1'
IDENTITY = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
DEFAULT_LOCATIONS = {
    'DIST_University': 'gamestart',               # retail career start
    'DIST_Industrial': 'dist_industrial_reclaimed',  # Haystings Park
    'DIST_DownTown': 'dist_downtown_skatepark',   # Rosalita Skate Park
    'DIST_MaloofMoneyCup': 'dist_maloof_street',  # street course
}


def heading(forward):
    """Game convention: forward = (sin h, 0, cos h) (Mat3::from_rotation_y)."""
    return math.atan2(forward[0], forward[2])


def locators(game_root):
    found = {}
    for path in sorted((Path(game_root) / 'data/content/global_locators').rglob('*.rx2')):
        for record in location_records(path.read_bytes()):
            found.setdefault((path.parent.name.casefold(), record['locator']), []).append(record['matrix'])
    return found


def all_locators(game_root, dlc_vaults=()):
    """{(district folder casefolded, locator): [matrices]}: the disc's global locators, the packs' and, for names those
    lack, each DLC stream's own _Sim.loc (its copies of shared names differ slightly)."""
    records = locators(game_root)
    for v in dlc_vaults:
        for key, matrices in v['locators'].items():
            records.setdefault(key, []).extend(matrices)
    for v in dlc_vaults:
        for key, matrices in v.get('stream_locators', {}).items():
            if key not in records:
                records[key] = list(matrices)
    return records


def prepare(game_root, converted, dlc_vaults=()):
    """Return {district: {locator, source, position, heading}}. `dlc_vaults` (asset_pipeline.dlc.vaults): the player's
    DLC rows join the disc's, each resolving its text against its own vault's .bin, with the packs' locators."""
    game_root = Path(game_root)
    from .dlc import merged
    collections = Collections(merged(converted, list(dlc_vaults)))
    binaries = {v['vault']: v['binary'] for v in dlc_vaults}
    database = BigArchive(game_root / 'data/big/db.big')
    with tempfile.TemporaryDirectory(prefix='skate-map-starts-') as tmp:
        entries = [e for e in database.entries if e.path in
                   ('data/db/skatercollections.bin', 'data/db/skatercollections.vlt')]
        database.extract_entries(entries, Path(tmp))
        _, binary, _ = vault(Path(tmp) / 'data/db/skatercollections')

    def stream_of(world_key):
        world, _ = collections.resolve('world', world_key)
        value = world.get(key_hash('WorldStream'))
        return value['data'] if value else None

    wanted = {}
    fe_rows = {}
    for (cls, key), row in collections.rows.items():
        if cls == key_hash('world'):
            fields, _ = collections.resolve('world', key)
            start, stream = fields.get(key_hash(START_LOCATOR)), stream_of(key)
            if start and stream and stream.lower().startswith('dist_'):
                # Parks list several world rows (full/empty/tutorial); the
                # stream's own row is the one named after it.
                if row['key'].casefold() == ('dist_' + stream[5:]).casefold() or stream not in wanted:
                    wanted[stream] = [(start['data'], 'world ' + row['key'])]
        elif cls == key_hash('fe_locations'):
            fields, _ = collections.resolve(cls, key)
            own = binaries.get(row.get('vault'), binary)
            location = cstring(own, int(fields[key_hash('location')]['data'], 16)).decode('utf-8')
            world = fields.get(key_hash('World'))
            stream = stream_of(int(world['data'][:16], 16)) if world else None
            fe_rows[row['key']] = (location, stream)
    for district, key in DEFAULT_LOCATIONS.items():
        if key in fe_rows:
            wanted.setdefault(district, []).append((fe_rows[key][0], 'fe_locations ' + key))
    by_stream = {}
    for key, (location, stream) in fe_rows.items():
        if stream:
            by_stream.setdefault(stream, []).append((location, key))
    for stream, rows in by_stream.items():
        if len({location for location, _ in rows}) == 1 and rows[0][0] not in (c for c, _ in wanted.get(stream, [])):
            wanted.setdefault(stream, []).append((rows[0][0], 'fe_locations ' + rows[0][1]))

    records = all_locators(game_root, dlc_vaults)
    result = {}
    for district, candidates in sorted(wanted.items()):
        start = pick(district, candidates, records)
        if start:
            result[district] = start
    return result


def pick(district, candidates, records):
    """The first candidate naming one placed locator; None (geometric rule) when every one is missing or ambiguous."""
    for locator, source in candidates:
        matrices = records.get((district.casefold(), locator), [])
        if len({json.dumps(m) for m in matrices}) != 1 or matrices[0] == IDENTITY:
            continue  # identity = never placed (Art Gallery's world start z_ArtGallery in DIST_AGPark_Sim.loc)
        m = matrices[0]
        return dict(locator=locator, source=source, position=[float(v) for v in m[3][:3]], heading=heading(m[2]))
    return None
