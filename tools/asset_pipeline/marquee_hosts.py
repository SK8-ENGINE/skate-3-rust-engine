"""Challenge hosts (the standing pros) from the retail challenge bank.

Retail (research b129, b96): the challenge manager (`sub_82553DD0` -> `sub_825543E0`) walks each challenge
record's `tGlobalSpawnCharacterInfo` array (field 555C505A36C3353A, 40-byte elements: +0 class key
`livingworld_entities`, +8 entity key, +24 locator name pointer, +28 flag) and places the host at the named
locator: `sub_8296EA10` hashes the raw name, the LocationManager registry finds the record and `sub_82911008`
returns its +48 vector (matrix row 3). A name the registry does not hold stays in a pending list (`sub_82557228`),
so it is exported with no matrix. Names match exactly (the hash is case-sensitive on the bytes).
The output stays in the private install; nothing here is committed.
"""
import json
import struct
import tempfile
from pathlib import Path
from tools.owned_game.big import BigArchive
from .teleports import cstring, location_records
from .vlt import convert as convert_vlt, hash64, vault

CHALLENGE_CLASS = 'Hash_A4F35B159D7432B5'
HOSTS_FIELD = 'Hash_555C505A36C3353A'
ELEMENT_SIZE = 40
VERSION = 1


def elements(field):
    """The 40-byte elements of a `tGlobalSpawnCharacterInfo` array: (entity key, name pointer, flag)."""
    out = []
    for item in field.get('array', {}).get('items', []):
        raw = bytes.fromhex(''.join(item.split()))
        if len(raw) < ELEMENT_SIZE:
            raise ValueError('Short challenge host element')
        entity, = struct.unpack_from('>Q', raw, 8)
        pointer, flag = struct.unpack_from('>2I', raw, 24)
        out.append((entity, pointer, flag))
    return out


def hosts(records, binary, locators, entities=()):
    """One row per host element of every challenge record with a locator name. `locators` maps an exact
    name to its records ({'map', 'matrix'}); a name with several different matrices is an error.
    `entities`: the `livingworld_entities` names; the element's key is hash64(name)."""
    by_key = {hash64(n): n for n in entities}
    rows = []
    for record in records:
        if record['class'] != CHALLENGE_CLASS or HOSTS_FIELD not in record['fields']:
            continue
        for index, (entity, pointer, flag) in enumerate(elements(record['fields'][HOSTS_FIELD])):
            name = cstring(binary, pointer).decode('utf-8') if pointer else ''
            if not name:
                continue  # Base templates (`contests`, `photos`, ...) carry an empty name.
            matches = locators.get(name, [])
            if len({json.dumps(m['matrix']) for m in matches}) > 1:
                raise ValueError('Ambiguous challenge host locator: ' + name)
            row = {'challenge': record['key'], 'element': index, 'entity': '%016X' % entity,
                   'entity_name': by_key.get(entity),
                   'locator': name, 'flag': flag, 'map': None, 'matrix': None}
            if matches:
                row.update(map=matches[0]['map'], matrix=matches[0]['matrix'])
            rows.append(row)
    rows.sort(key=lambda r: (r['challenge'], r['element']))
    return rows


def export(game_root, output, names):
    """Writes `marquee_hosts.json` into the living world output; returns its summary."""
    game_root, output = Path(game_root), Path(output)
    database = BigArchive(game_root / 'data/big/db.big')
    wanted = {'data/db/skaterschema.bin', 'data/db/skaterschema.vlt',
              'data/db/challengebanks/main.bin', 'data/db/challengebanks/main.vlt'}
    with tempfile.TemporaryDirectory(prefix='skate-hosts-') as tmp:
        root = Path(tmp)
        database.extract_entries([e for e in database.entries if e.path.lower() in wanted], root)
        converted = convert_vlt(root / 'data/db/skaterschema', root / 'data/db/challengebanks/main', names)
        _, binary, _ = vault(root / 'data/db/challengebanks/main')
    locators = {}
    for path in sorted((game_root / 'data/content/global_locators').rglob('*.rx2')):
        for record in location_records(path.read_bytes()):
            locators.setdefault(record['locator'], []).append(
                {'map': path.parent.name.removeprefix('DIST_'), 'matrix': record['matrix']})
    tables = output / 'tables.json'
    entities = json.loads(tables.read_text(encoding='utf-8'))['classes']['livingworld_entities'] if tables.exists() else {}
    rows = hosts(converted['collections'], binary, locators, entities)
    output.mkdir(parents=True, exist_ok=True)
    (output / 'marquee_hosts.json').write_text(json.dumps({'version': VERSION, 'hosts': rows}, indent=1),
                                              encoding='utf-8')
    return {'hosts': len(rows), 'placed': sum(1 for r in rows if r['matrix'] is not None)}
