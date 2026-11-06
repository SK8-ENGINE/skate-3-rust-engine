"""Challenges per map from retail data (USER-ASKS 498; research b826, b834, b835; doc 42).

A retail challenge is a row of the challenge bank (`data/db/challengebanks/main`, class challenge_global_data) that
inherits its family's defaults through its parent chain (`ots_dwtn_01` < `own_the_spots`). Per challenge:
- the bank row: OTSTriggerBoundary (the challenge boundary volume), ObjectivesOwned / ObjectivesKilledIt (RefSpecs to
  challenge_objectives_group rows whose PointRequirement is the owned it / killed it score), title and description ids;
- the local data vault (`data/db/challenge_local_data/<key>`): OTSScoringBoundary (the spot) and StartLocation;
- the hull (`data/content/missions/<key>/`): the volumes and the start locator (0x00EB0009 records, research b835).
Only the families in FAMILIES are exported. A challenge whose volumes or start do not resolve is left out and
reported. Output: assets/private/challenges.json.
"""
import json
import re
import struct
import sys
from pathlib import Path

from .teleporters import _sources, parse_trigger, LOCAL, BANKS, MISSIONS
from .teleports import cstring, label_of, location_records
from .vlt import hash64

FORMAT = 'skate3rust.challenges'
VERSION = 1
FAMILIES = ('own_the_spots',)
CHALLENGE = 'Hash_A4F35B159D7432B5'  # challenge_global_data
GROUP = 'Hash_3CDBD8935AB90C06'      # challenge_objectives_group


def _field(name):
    return 'Hash_%016X' % hash64(name)


TRIGGER_BOUNDARY = _field('OTSTriggerBoundary')
SCORING_BOUNDARY = _field('OTSScoringBoundary')
START = _field('StartLocation')
OWNED = _field('ObjectivesOwned')
KILLED = _field('ObjectivesKilledIt')
POINTS = _field('PointRequirement')
OBJECTIVES = _field('Objectives')
DEFINITION = 'Hash_E13D9C326E4D98D5'  # tObjectiveDefinition: completion condition text offset, description offset
ENTERED = re.compile(r'EnteredVolume\(\s*"(\d+)"\s*,\s*"([^"|]+)\|([^"]+)"')
SIGN_UP_RADIUS = _field('GlobalActivateRadius')
DISCOVERY_RADIUS = _field('DiscoveryActivateRadius')
TITLE = 'Hash_E2A50788B6C20C92'
DESCRIPTION = 'Hash_6B0BD2BAE508B742'


def chain(rows, cls, key):
    """The row's fields merged with its parents' (own value first), and the family keys walked."""
    fields, walked, key = {}, [], key.casefold()
    while key and key not in walked and (cls, key) in rows:
        walked.append(key)
        parent, own = rows[cls, key]
        for name, value in own.items():
            fields.setdefault(name, value)
        key = parent.casefold()
    return fields, walked


def group_points(rows_by_hash, value):
    """PointRequirement of the first objectives group a RefSpec array names, or None."""
    items = ((value or {}).get('array') or {}).get('items') or []
    for item in items:
        raw = bytes.fromhex(item)
        if len(raw) < 16:
            continue
        cls, key = struct.unpack_from('>QQ', raw, 0)
        row = rows_by_hash.get(('Hash_%016X' % cls, key))
        if row and POINTS in row['fields']:
            return struct.unpack('>i', bytes.fromhex(row['fields'][POINTS]['data']))[0]
    return None


def location_field(fields):
    """The bank row's Location (tLocationID), under its name or its hash."""
    return fields.get('Location') or fields.get(_field('Location'))


def radius(fields, key):
    value = fields.get(key)
    return struct.unpack('>f', bytes.fromhex(value['data'][:8]))[0] if value and value.get('data') else None


def refs(value):
    """(class, key hash) of each RefSpec array item."""
    out = []
    for item in ((value or {}).get('array') or {}).get('items') or []:
        raw = bytes.fromhex(item)
        if len(raw) >= 16:
            out.append(('Hash_%016X' % struct.unpack_from('>Q', raw, 0)[0], struct.unpack_from('>Q', raw, 8)[0]))
    return out


def spot_volume(rows_by_hash, owned, binary):
    """(district, volume name, instance id) the owned group's spot objective completes on: its completion condition
    is Lua text in the bank (research b840) calling ObjectiveTrigger.EnteredVolume("<id>", "DISTRICT|name")."""
    for group in refs(owned):
        row = rows_by_hash.get(group)
        for objective in refs((row or {}).get('fields', {}).get(OBJECTIVES)):
            definition = (rows_by_hash.get(objective) or {}).get('fields', {}).get(DEFINITION)
            if not definition:
                continue
            text = cstring(binary, struct.unpack_from('>I', bytes.fromhex(definition['data']), 0)[0]).decode('utf-8')
            match = ENTERED.search(text)
            if match:
                return match.group(2), match.group(3), int(match.group(1))
    return None


def export(game_root, dlc_vaults, dlc_bigs, work, output, text=None, report=print):
    """Writes `output` (challenges.json) and returns its challenges."""
    from .vlt import convert, vault
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'vendor/university/tools/vanilla_map_extraction/tools'))
    from .map_volumes import district_volumes, stream_arenas
    work = Path(work)
    text = text or {}
    names = (Path(__file__).resolve().parent / 'names.txt').read_text(encoding='utf-8').splitlines()

    local, banks, hulls = {}, [], {}
    for source, archive in _sources(game_root, dlc_bigs):
        entries = {e.path.replace('\\', '/').lower(): e for e in archive.entries}
        for path, entry in entries.items():
            if path.startswith(LOCAL) and path.endswith('.vlt') and path[:-4] + '.bin' in entries:
                local.setdefault(path[len(LOCAL):-4], (source, archive, entry, entries[path[:-4] + '.bin']))
            elif path.startswith(BANKS) and path.endswith('.vlt') and path[:-4] + '.bin' in entries:
                banks.append((source, archive, entry, entries[path[:-4] + '.bin']))
            elif path.startswith(MISSIONS) and path.count('/') == 4:
                hulls.setdefault(path.split('/')[3], (archive, []))[1].append(entry)
    schema = work / 'schema'
    if not (schema / 'data/db/skaterschema.vlt').is_file():
        disc = next(a for s, a in _sources(game_root, ()))
        disc.extract_entries([e for e in disc.entries
                              if Path(e.path).name.lower() in {'skaterschema.bin', 'skaterschema.vlt'}], schema)

    def read_vault(archive, vlt_entry, bin_entry, folder):
        stem = work / folder / Path(vlt_entry.path.replace('\\', '/')).stem
        stem.parent.mkdir(parents=True, exist_ok=True)
        for entry, suffix in ((vlt_entry, '.vlt'), (bin_entry, '.bin')):
            stem.with_suffix(suffix).write_bytes(archive.read(entry))
        return convert(schema / 'data/db/skaterschema', stem, names), vault(stem)[1]

    rows, rows_by_hash, origin = {}, {}, {}
    for index, (source, archive, vlt_entry, bin_entry) in enumerate(banks):
        data, binary = read_vault(archive, vlt_entry, bin_entry, f'banks/{index}')
        for row in data['collections']:
            key = (row['class'], row['key'].casefold())
            if key in rows:
                continue
            rows[key] = (row['parent'], row['fields'])
            # A row whose key string is not in names.txt keeps its hash as `Hash_<16 hex>` (the killed it groups).
            hashed = int(row['key'][5:], 16) if row['key'].startswith('Hash_') else hash64(row['key'])
            rows_by_hash[row['class'], hashed] = row
            origin[key] = (source, binary)

    cache = {}

    def global_locators():
        from .map_starts import all_locators
        if 'records' not in cache:
            cache['records'] = all_locators(game_root, dlc_vaults)
        return cache['records']

    def volume_of(folder, instance):
        return next((v for v in district_volumes(folder, strict=False) if v['instance_id'] == '%016x' % instance), None)

    result, skipped = [], []
    for (cls, key), (parent, _) in sorted(rows.items()):
        if cls != CHALLENGE or key.endswith('_ol'):
            continue
        fields, walked = chain(rows, cls, key)
        family = next((f for f in FAMILIES if f in walked[1:]), None)
        if family is None or key in FAMILIES:
            continue
        source, bank_binary = origin[cls, key]
        try:
            if TRIGGER_BOUNDARY not in fields:
                raise ValueError('no OTSTriggerBoundary')
            district, boundary_name, boundary_id = parse_trigger(
                bytes.fromhex(fields[TRIGGER_BOUNDARY]['data']), bank_binary)
            if key not in local:
                raise ValueError('no local data vault')
            _, archive, vlt_entry, bin_entry = local[key]
            data, binary = read_vault(archive, vlt_entry, bin_entry, 'local')
            own = next(r for r in data['collections'] if SCORING_BOUNDARY in r['fields'])['fields']
            _, spot_name, spot_id = parse_trigger(bytes.fromhex(own[SCORING_BOUNDARY]['data']), binary)
            if START not in own:
                raise ValueError('no StartLocation')  # ots_ind_01: a leftover beside ots_indu_01..05
            start_name = cstring(binary, int(own[START]['data'], 16)).decode('utf-8')
            if key not in hulls:
                raise ValueError('no hull')
            hull_archive, entries = hulls[key]
            folder = work / 'hulls' / MISSIONS / key
            if not folder.is_dir():
                hull_archive.extract_entries(entries, work / 'hulls')
            target = spot_volume(rows_by_hash, fields.get(OWNED), bank_binary)
            if target is None:
                raise ValueError('no spot objective volume')
            boundary, scoring = volume_of(folder, boundary_id), volume_of(folder, spot_id)
            spot = volume_of(folder, target[2])
            if boundary is None or scoring is None or spot is None:
                raise ValueError('hull lacks a volume')
            starts = [r['matrix'] for _, _, arena in stream_arenas(folder) for r in location_records(arena)
                      if r['locator'] == start_name]
            if not starts:  # a few starts sit in the district's global locators instead of the hull
                starts = global_locators().get((district.casefold(), start_name), [])
            if len({json.dumps(m) for m in starts}) != 1:
                raise ValueError(f'start locator {start_name}: {len(starts)} records')
            location = location_field(fields)
            if not location:
                raise ValueError('no Location')
            location_name = cstring(bank_binary, int(location['data'], 16)).decode('utf-8')
            places = global_locators().get((district.casefold(), location_name), [])
            if not places:
                places = [r['matrix'] for _, _, arena in stream_arenas(folder) for r in location_records(arena)
                          if r['locator'] == location_name]
            if len({json.dumps(m) for m in places}) != 1:
                raise ValueError(f'location {location_name}: {len(places)} records')
            sign_up, discovery = radius(fields, SIGN_UP_RADIUS), radius(fields, DISCOVERY_RADIUS)
            if sign_up is None or discovery is None:
                raise ValueError('no activation radii')
            owned = group_points(rows_by_hash, fields.get(OWNED))
            killed = group_points(rows_by_hash, fields.get(KILLED))
            if owned is None or killed is None:
                raise ValueError('objectives group without PointRequirement')
        except (KeyError, ValueError, StopIteration) as error:
            skipped.append(f'{key} ({error})')
            continue
        title_id = (fields.get(TITLE) or {}).get('data') or None
        result.append(dict(
            id=key, family=family, source=source, map=label_of(district), district=district,
            title_id=title_id, title=text.get(title_id) if title_id else None,
            description_id=(fields.get(DESCRIPTION) or {}).get('data') or None,
            boundary=dict(name=boundary_name, id='%016x' % boundary_id, volume=boundary),
            scoring=dict(name=spot_name, id='%016x' % spot_id, volume=scoring),
            spot=dict(name=target[1], id='%016x' % target[2], volume=spot),
            start=dict(locator=start_name, matrix=starts[0]),
            location=dict(locator=location_name, matrix=places[0]),
            sign_up_radius=sign_up, discovery_radius=discovery,
            owned_points=owned, killed_points=killed))
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({'format': FORMAT, 'version': VERSION, 'challenges': result}, indent=1),
                      encoding='utf-8')
    report(f'Challenges: {len(result)} ({", ".join(FAMILIES)}) on {len({c["map"] for c in result})} maps'
           + (f'; left out: {", ".join(skipped)}' if skipped else ''))
    return result
