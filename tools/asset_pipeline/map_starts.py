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


def prepare(game_root, converted):
    """Return {district: {locator, source, position, heading}}."""
    game_root = Path(game_root)
    collections = Collections(converted)
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
            if start and stream and stream.startswith('DIST_'):
                # Parks list several world rows (full/empty/tutorial); the
                # stream's own row is the one named after it.
                if row['key'].casefold() == ('dist_' + stream[5:]).casefold() or stream not in wanted:
                    wanted[stream] = (start['data'], 'world ' + row['key'])
        elif cls == key_hash('fe_locations'):
            fields, _ = collections.resolve(cls, key)
            location = cstring(binary, int(fields[key_hash('location')]['data'], 16)).decode('utf-8')
            world = fields.get(key_hash('World'))
            stream = stream_of(int(world['data'][:16], 16)) if world else None
            fe_rows[row['key']] = (location, stream)
    for district, key in DEFAULT_LOCATIONS.items():
        if district not in wanted and key in fe_rows:
            wanted[district] = (fe_rows[key][0], 'fe_locations ' + key)
    by_stream = {}
    for key, (location, stream) in fe_rows.items():
        if stream:
            by_stream.setdefault(stream, []).append((location, key))
    for stream, rows in by_stream.items():
        if stream not in wanted and len({location for location, _ in rows}) == 1:
            wanted[stream] = (rows[0][0], 'fe_locations ' + rows[0][1])

    records = locators(game_root)
    result = {}
    for district, (locator, source) in sorted(wanted.items()):
        matrices = records.get((district.casefold(), locator), [])
        if len({json.dumps(m) for m in matrices}) != 1:
            continue  # missing or ambiguous: keep the geometric rule
        m = matrices[0]
        result[district] = dict(locator=locator, source=source,
                                position=[float(v) for v in m[3][:3]], heading=heading(m[2]))
    return result
