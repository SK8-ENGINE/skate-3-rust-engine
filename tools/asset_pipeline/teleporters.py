"""Teleporters per map from retail data (research b420 / b421 / b423; disc and DLC, USER-ASKS 167 / 172).

A retail teleporter is a challenge of the `teleports` group (stategraph challenge_stategraph/teleport):
- its bank row (`data/db/challengebanks/*`, keyed by the challenge) carries the title the on-screen prompt shows;
- its local data vault (`data/db/challenge_local_data/<challenge>`) carries TeleporterTrigger, the text
  "DISTRICT|volume|0xid" plus the volume's instance id, and TeleportExitLocation, a list of tWorldLocationID (the
  target `world` row and, at +24, the locator name in the vault's .bin);
- the volume itself sits in the challenge's hull (`data/content/missions/<challenge>/cSim_Global.xsf`) or, for a few,
  in the district's own world stream (then the map's .triggers sidecar already has it; `volume` stays None).
Exits resolve against the disc's and the packs' locators (map_starts.all_locators). A teleporter whose volume, map or
exits do not resolve is left out and reported. Output: assets/private/teleporters.json.
"""
import json
import struct
import sys
from pathlib import Path

from .environment import Collections, key_hash
from .teleports import cstring, label_of

FORMAT = 'skate3rust.teleporters'
VERSION = 1
TRIGGER = 'Hash_DA9330F647B19DF2'   # Sk8::Challenge::tTriggerVolumeInstanceID
EXITS = 'Hash_E7D448870EC1A6E9'     # Sk8::Challenge::tWorldLocationID[]
TITLE = 'Hash_E2A50788B6C20C92'     # the challenge's title text id (bank row)
LOCATOR = 'Hash_F1C6D5FA4A23951E'   # the challenge's locator (bank row, a string): its visual indicator's anchor (b533)
INDICATOR = 'Hash_284B07325D587CC4' # RefSpec -> ribbon_indicator row (bank row, inherited from the group, b532)
LOCAL = 'data/db/challenge_local_data/'
BANKS = 'data/db/challengebanks/'
MISSIONS = 'data/content/missions/'


def parse_trigger(raw, binary):
    """(district, volume name, instance id) from a TeleporterTrigger value (u32 name offset, pad, u64 id)."""
    if len(raw) < 16:
        raise ValueError('short teleporter trigger')
    text = cstring(binary, struct.unpack_from('>I', raw, 0)[0]).decode('utf-8')
    instance = struct.unpack_from('>Q', raw, 8)[0]
    parts = text.split('|')
    if len(parts) != 3 or int(parts[2], 16) != instance:
        raise ValueError('teleporter trigger text does not name its volume id: ' + text)
    return parts[0], parts[1], instance


def parse_exits(items, binary):
    """[(world row key hash, locator name)] from TeleportExitLocation items (32 bytes each)."""
    out = []
    for item in items:
        raw = bytes.fromhex(item)
        if len(raw) != 32:
            raise ValueError('teleport exit is not a 32-byte tWorldLocationID')
        world = struct.unpack_from('>Q', raw, 8)[0]
        out.append((world, cstring(binary, struct.unpack_from('>I', raw, 24)[0]).decode('utf-8')))
    return out


CHALLENGE = 'Hash_A4F35B159D7432B5'  # the challenge bank class


def indicator(bank_rows, challenge, ribbons, locate):
    """The challenge's visual indicator {key, locator, matrix} through its bank row's parent chain, None when it has
    none, or the reason (a string) when its locator does not resolve to one matrix (retail 82D53F70, research b533)."""
    fields, key, seen = {}, challenge.casefold(), set()
    while key and key not in seen and (CHALLENGE, key) in bank_rows:
        seen.add(key)
        parent, own = bank_rows[CHALLENGE, key]
        for name, value in own.items():
            fields.setdefault(name, value)
        key = parent.casefold()
    if 'indicator' not in fields or not fields.get('locator'):
        return None
    matrices = locate(fields['locator'])
    if len({json.dumps(m) for m in matrices}) != 1:
        return f'locator {fields["locator"]}: {len(matrices)} records'
    return dict(key=ribbons.get(fields['indicator'], 'Hash_%016X' % fields['indicator']), locator=fields['locator'],
                matrix=matrices[0])


def _sources(game_root, dlc_bigs):
    """Every archive that may hold teleporter data: the disc's db.big and missions.big, then the packs' BIGs."""
    from tools.owned_game.big import BigArchive
    game_root = Path(game_root)
    yield 'disc', BigArchive(game_root / 'data/big/db.big')
    yield 'disc', BigArchive(game_root / 'data/content/missions.big')
    for big in dlc_bigs:
        yield 'dlc:' + Path(big).name, BigArchive(big)


def export(game_root, converted, dlc_vaults, dlc_bigs, work, output, text=None, report=print):
    """Writes `output` (teleporters.json) and returns its teleporters."""
    from .map_starts import all_locators
    from .vlt import convert, vault
    from .dlc import merged
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'vendor/university/tools/vanilla_map_extraction/tools'))
    from .map_volumes import district_volumes
    work = Path(work)
    text = text or {}
    names = (Path(__file__).resolve().parent / 'names.txt').read_text(encoding='utf-8').splitlines()
    collections = Collections(merged(converted, list(dlc_vaults)))
    worlds, world_names = {}, {}
    for (cls, key), row in collections.rows.items():
        if cls == key_hash('world'):
            stream = collections.resolve('world', key)[0].get(key_hash('WorldStream'))
            if stream:
                worlds[key] = stream['data']
                world_names[key] = row['key']
    records = all_locators(game_root, dlc_vaults)

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

    titles, bank_rows = {}, {}
    for _, archive, vlt_entry, bin_entry in banks:
        converted_bank, bank_binary = read_vault(archive, vlt_entry, bin_entry, 'banks')
        for row in converted_bank['collections']:
            title = row['fields'].get(TITLE)
            if title and title.get('data') and not title['data'].startswith('#'):
                titles.setdefault(row['key'].casefold(), title['data'])
            fields = {}
            if row['fields'].get(LOCATOR, {}).get('data'):
                fields['locator'] = cstring(bank_binary, int(row['fields'][LOCATOR]['data'], 16)).decode('utf-8')
            if row['fields'].get(INDICATOR, {}).get('data'):
                fields['indicator'] = struct.unpack_from('>Q', bytes.fromhex(row['fields'][INDICATOR]['data']), 8)[0]
            bank_rows.setdefault((row['class'], row['key'].casefold()), (row['parent'], fields))
    ribbons = {k: r['key'] for (c, k), r in collections.rows.items() if c == key_hash('ribbon_indicator')}

    result, skipped, unreadable, no_marker = [], [], [], []
    for challenge, (source, archive, vlt_entry, bin_entry) in sorted(local.items()):
        try:
            data, binary = read_vault(archive, vlt_entry, bin_entry, 'local')
        except (KeyError, ValueError):
            unreadable.append(challenge)  # mostly other challenge kinds the schema reader cannot lay out yet
            continue
        # A pack's vault also carries its parents (`default`, the group row `dlc_dwgh_teleports` with an empty
        # trigger, id 0): the teleporter is the row whose trigger names a volume.
        row = next((r for r in data['collections'] if TRIGGER in r['fields'] and EXITS in r['fields']
                    and bytes.fromhex(r['fields'][TRIGGER]['data'])[8:16].strip(bytes(1))), None)
        if row is None:
            continue  # another kind of challenge, or a group template
        try:
            district, volume_name, instance = parse_trigger(bytes.fromhex(row['fields'][TRIGGER]['data']), binary)
            items = (row['fields'][EXITS].get('array') or {}).get('items') or []
            exits = []
            for world, locator in parse_exits(items, binary):
                stream = worlds.get(world)
                if not stream:
                    raise ValueError(f'exit world {world:016x} names no stream')
                matrices = records.get((stream.casefold(), locator), [])
                if len({json.dumps(m) for m in matrices}) != 1:
                    raise ValueError(f'exit locator {locator} in {stream}: {len(matrices)} records')
                # The exit's world row (a skate.Park exit names its Full world, `dist_megapark_full`: research b532).
                exits.append(dict(map=label_of(stream), district=stream, world=world_names[world], locator=locator, matrix=matrices[0]))
            if not exits:
                raise ValueError('no exits')
            volume = None
            if challenge in hulls:
                hull_archive, entries = hulls[challenge]
                folder = work / 'hulls' / MISSIONS / challenge
                if not folder.is_dir():
                    hull_archive.extract_entries(entries, work / 'hulls')
                volume = next((v for v in district_volumes(folder) if v['instance_id'] == '%016x' % instance), None)
                if volume is None:
                    raise ValueError(f'hull {challenge} lacks volume {instance:016x}')
                volume['stream'] = 'missions/' + challenge
        except (KeyError, ValueError, StopIteration) as error:
            skipped.append(f'{challenge} ({error})')
            continue
        title_id = titles.get(challenge.casefold())
        marker = indicator(bank_rows, challenge, ribbons, lambda name: records.get((district.casefold(), name), []))
        if isinstance(marker, str):
            no_marker.append(f'{challenge} ({marker})')
            marker = None
        result.append(dict(id=challenge, source=source, map=label_of(district), district=district,
                           volume_name=volume_name, volume_id='%016x' % instance, volume=volume,
                           title_id=title_id, title=text.get(title_id) if title_id else None, exits=exits,
                           indicator=marker))
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({'format': FORMAT, 'version': VERSION, 'teleporters': result}, indent=1),
                      encoding='utf-8')
    tele = [c for c in unreadable if c.startswith('tele')]
    report(f'Teleporters: {len(result)} on {len({t["map"] for t in result})} maps'
           + (f'; left out: {", ".join(skipped)}' if skipped else '')
           + (f'; no indicator: {", ".join(no_marker)}' if no_marker else '')
           + (f'; {len(unreadable)} other challenge vaults unreadable' if unreadable else '')
           + (f' (named like teleporters: {", ".join(tele)})' if tele else ''))
    return result
