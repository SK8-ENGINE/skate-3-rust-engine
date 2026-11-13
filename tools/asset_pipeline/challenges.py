"""Challenges per map from retail data (USER-ASKS 498; research b826, b834, b835; doc 42).

A retail challenge is a row of the challenge bank (`data/db/challengebanks/main`, class challenge_global_data) that
inherits its family's defaults through its parent chain (`ots_dwtn_01` < `own_the_spots`). Per challenge:
- the bank row: OTSTriggerBoundary (the challenge boundary volume), ObjectivesOwned / ObjectivesKilledIt (RefSpecs to
  challenge_objectives_group rows whose PointRequirement is the owned it / killed it score), title and description ids;
- the local data vault (`data/db/challenge_local_data/<key>`): OTSScoringBoundary (the spot) and StartLocation; its
  row (class challenge parameters) inherits from the framework vault (`data/db/challenge_local_data_framework`,
  family row, then `default`): OutOfBoundsTime, the owned it / killed it banner times and the return to area
  message id (research b847);
- the hull (`data/content/missions/<key>/`): the volumes and the start locator (0x00EB0009 records, research b835).
Only the families in FAMILIES are exported. A challenge whose volumes or start do not resolve is left out and
reported. Output: assets/private/challenges.json.
"""
import json
import os
import re
import struct
import sys
from pathlib import Path

from .teleporters import _sources, parse_trigger, LOCAL, BANKS, MISSIONS
from .teleports import cstring, label_of, location_records, locator_slots
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
# Failure conditions (research b938 / b940): FailureObjectivesWithEvents rows (each a Lua predicate plus its own
# PresentationEvents, played when it fires); EvaluateFailureBeforeCompletionObjectives orders the checks.
FAILURES_WITH_EVENTS = 'Hash_B69AF085F7A6D275'
PRESENTATION_EVENTS = 'Hash_BD96B775B855E129'
FAILURE_FIRST = 'Hash_B053106604767956'
FAILURES_INLINE = 'Hash_40D40861DD9A6CC6'  # FailureObjectives: inline tObjectiveDefinition list (b938 / b940)
# HintObjectives (82C755B8, research b942): the same row shape as the failure rows.
HINTS = 'Hash_ECAEB8E619A68AF3'
DEFINITION = 'Hash_E13D9C326E4D98D5'  # tObjectiveDefinition: completion condition text offset, description offset
POINTS_VALID = 'Hash_41CBF3F0889DE6B3'  # hash64("PointsValid"), tCompiledLua: text offset first (b840)
TRIGGERS = 'Hash_D4965ED3BD43EC44'  # tObjectiveTriggers: bytes 0..13 = listens to message kind N (b840)
ACTIVATION_MODE = 'Hash_D7332063BE5CC0EB'  # eTriggerActivationMode u32 (0 at once, 2 / 3 volume, 4 wipeout, 5 sequence)
# Tutorials (research b858): the family row, the local data fields the `tutorials` graph reads, and the row's world.
TUTORIAL = 'tutorial'
SPAWN_OFFBOARD = _field('SpawnOffboard')
# CompleteChallenge's re-activate flag for tutorials (research b918 / b919): true only on tut_skpk_01..05.
PERSISTENT = _field('Persistent')
PRESENTATION = {'intro': _field('IntroPresentationEvents'), 'outro': _field('OutroPresentationEvents'),
                'restart': _field('RestartPresentationEvents')}
IGNORE_REPOSITION = _field('IgnoreInitialTutorialReposition')
WORLD = 'Hash_7565CD2A80E93A5E'  # ClassRefSpec_world, key first (dist_skateschool for the school rows)
ENTERED = re.compile(r'EnteredVolume\(\s*"(\d+)"\s*,\s*"([^"|]+)\|([^"]+)"')
SIGN_UP_RADIUS = _field('GlobalActivateRadius')
DISCOVERY_RADIUS = _field('DiscoveryActivateRadius')
PARAMS = 'Hash_46A56ADDF284C381'     # the challenge parameters (local data and framework rows)
FRAMEWORK = 'data/db/challenge_local_data_framework'
# Own the lots (research b905 / b916 / b922): ChallengeType 0x34 rows, their child list (RefSpec array, key at bytes
# 8..15, looked up in challenge_global_data) and each child's FreeskateActivityType (eFreeskateType, inherited).
LOT_TYPE = 0x34
LOT_CHILDREN = 'Hash_ABE749C80E8ADBCE'
ACTIVITY_TYPE = 'Hash_85D6B2C6AE61DD8B'
DEBUG_ONLY = 'Hash_FD40E4A31027C984'
OUT_OF_BOUNDS_TIME = _field('OutOfBoundsTime')
OWNED_TIME = _field('OwnedItHUDDisplayTime')
KILLED_TIME = _field('KilledItHUDDisplayTime')
RETURN_MESSAGE = _field('CentralMessageHALIDReturnToChallengeArea')
CHALLENGE_TYPE = _field('ChallengeType')  # eChallengeTypes (own the spot 9)
TITLE = 'Hash_E2A50788B6C20C92'
DESCRIPTION = 'Hash_6B0BD2BAE508B742'
# The sign-up poster (research b947): the description's one "%d" argument (Int8, default 0) and IsObjectivesEnabled
# (Bool, default false).
DESCRIPTION_ARG = 'Hash_0AF3F024F18EAB9D'
OBJECTIVES_ENABLED = 'Hash_858D73CD6085C8EC'
# Races (research b979): params row RaceHeats -> challenge_race_heats -> challenge_race_legs (split triggers, gates
# -> challenge_race_gates), all inside the race's local vault; competitors on the bank row.
RACES = 'races'
RACE_HEATS = _field('RaceHeats')
LEGS = _field('Legs')
SPLITS = _field('SplitTimeTriggers')
GATES = 'Hash_819849159B9E26AD'
GATE_VOLUME = 'Hash_013BDB7AF1756799'
GATE_INDICATOR = 'Hash_B1BF3FF96CC7F6AD'
# Race gate presentation (research race-gate-indicators.md): the gate's ribbon (tSplineBankObject = a ribbon_indicator
# RefSpec + a ribbon_spline_colour RefSpec), Time_Bonus, gate audio; the race's skipable flag, minimap icon cap, gray
# icon and the gray / final / leg finish ribbons (absent rows: the game's retail defaults).
GATE_RIBBON = 'Hash_E02F0BA9F04FFF16'
GATE_TIME_BONUS = 'Hash_881D391E8B51AFE5'
GATE_AUDIO = 'Hash_DC41AFD60FDAD913'
RACE_SKIPABLE = 'Hash_94CAC0A22ABC4647'
RACE_MAX_MINIMAP_ICONS = 'Hash_F9B0D8DD44ADA713'
RACE_GREY_ICON = 'Hash_21236A5259312838'
RACE_GREY_RIBBON = 'Hash_EF0F24D18958E86A'
RACE_FINAL_RIBBON = 'Hash_9FCEC885B237766D'
RACE_LEG_FINISH_RIBBON = 'Hash_753B2FE8A30EADEA'
TIME_LIMIT = 'Hash_F89BFDFC88990F17'
TIME_LIMIT_WARNING = 'Hash_270125BF89FBBFDE'
KILLED_IT_TIME = _field('KilledItTime')
AI_PATH_INDEX_OVERRIDE = 'Hash_A2D9488ABA12559C'
AI_PATHS = 'Hash_0D46945C37C9C237'  # tRoundPaths: 200 u32 text offsets, u32 all paths (+0x320), u32 (+0x324)
END_TIME_BONUS = 'Hash_5FB1236FD2CB7556'
COMPETITORS = _field('Competitors')
# Contests and demos (research b877 / b1009, field names matched by hash 2026-11-11): the params row's ContestRounds
# (tContestRoundDefinition: a round row RefSpec + 8 bytes) -> round rows (class 1E04F6C0A209B06A) in the local vault.
CONTEST_FAMILIES = ('contests', 'demos')
CONTEST_ROUNDS = _field('ContestRounds')
ROUND_TIME = _field('RoundTime')
RUN_TYPE = _field('RunType')  # eRunType: 0 normal, 1 SoloRun, 3 TimedRunSingle, 4 TimedRunMulti, 5 (unnamed)
CONTEST_VARIANT = _field('ContestVariant')  # eContestType: the scorer (Sequence, Sequence_Grind, Air_Only, ...)
AI_SCORE_ADJUSTMENT = _field('AIScoreAdjustment')
CUMULATIVE = _field('CumulativeScoring')
CUMULATIVE_MIN = _field('CumulativeMinScore')
SOLO_RUN_NUM = _field('SoloRunNum')
SOLO_AI_MIN = _field('SoloRunAIScoreMin')  # Int16 (b877 read these as floats: 0)
SOLO_AI_MAX = _field('SoloRunAIScoreMax')
DEMO_TARGET = _field('DemoTargetScore')
WAIT_LOCATION = _field('WaitLocation')
ROUND_BOUNDARY = _field('RoundBoundary')
SPOT_VOLUMES = _field('SpotVolumes')
ROUND_PATHS = _field('Paths')
RESULT_COLUMNS = [_field('ResultsScreenHALIDCol%d' % i) for i in (1, 2, 3)]
MUSIC_TYPE = 'Hash_44754347A934FDCD'  # eChallengeMusicType (research music-challenges section 6)
MUSIC_PLAYLIST = 'Hash_874EC914B4D34708'  # the challenge's aud_playlists RefSpec (8248B258; default 614B4D4D82510720)
ONLINE_COMPETITORS = 'Hash_598056F37D38D27A'
# Follow the leader (research b875 / b1006): family follow_the_leaders (ChallengeType 0x17), run by objectives_shared.
FOLLOW = 'follow_the_leaders'
FTL_END_VOLUME = _field('FTLEndVolume')
FTL_MAX_DISTANCE = _field('FTLMaxFollowDistance')
CHALLENGE_BOUNDARY = _field('ChallengeBoundary')
COUNTDOWN_DELAY = _field('CountdownDelay')
PREFILM_TIME = _field('PrefilmTime')
TOTAL_FILM_TIME = _field('TotalFilmTime')
TIME_BEFORE_REPLAY = _field('TimeToWaitBeforeReplay')
ALLOW_PLAYER_BAIL = _field('AllowPlayerBail')
ALLOW_OBJECTIVE_FAILURE = _field('AllowObjectiveFailure')
ALLOW_SESSION_MARKERS = _field('AllowSessionMarkersToBeSet')
RESET_AI_ON_BAIL = _field('ResetAILinesOnBail')
RESET_AI_ON_MARKER = _field('ResetAILinesOnPlayerUseSessionMarker')
RESET_OBJECTIVES_ON_MARKER = _field('ResetObjectivesOnPlayerUseSessionMarker')


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


def key_hash(key):
    """A row key as its 64-bit hash (`Hash_<16 hex>` when its string is not in names.txt)."""
    return int(key[5:], 16) if key.startswith('Hash_') else hash64(key)


def parameters(row, framework):
    """The challenge parameters: the local row's fields, then its framework parents' (`framework`: hash ->
    (parent key, fields))."""
    fields, seen, parent = dict(row['fields']), set(), row['parent']
    while parent and key_hash(parent) not in seen and key_hash(parent) in framework:
        seen.add(key_hash(parent))
        parent, own = framework[key_hash(parent)]
        for name, value in own.items():
            fields.setdefault(name, value)
    out_of_bounds = fields.get(OUT_OF_BOUNDS_TIME)
    message = fields.get(RETURN_MESSAGE)
    return dict(
        out_of_bounds_time=struct.unpack('>i', bytes.fromhex(out_of_bounds['data']))[0] if out_of_bounds else None,
        owned_display_time=radius(fields, OWNED_TIME), killed_display_time=radius(fields, KILLED_TIME),
        return_to_area_message=(message or {}).get('data') or None)


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


def objective_programs(rows_by_hash, group, binary, binary_of=None):
    """Each objective of the group as {key, condition}: its completion condition translated from the bank's Lua text
    into the engine's data form (objective_lua.py; the port never runs retail Lua)."""
    out = []
    for group_ref in refs(group):
        row = rows_by_hash.get(group_ref)
        out.extend(objective_rows(rows_by_hash, refs((row or {}).get('fields', {}).get(OBJECTIVES)), binary, binary_of))
    return out


def objective_rows(rows_by_hash, objectives, binary, binary_of=None):
    """The objective rows `objectives` = [(class, key hash)] as exported programs (a tutorial lists them directly on
    its challenge row, own the spot through objective groups)."""
    from .objective_lua import translate
    out = []
    for objective in objectives:
        objective_row = rows_by_hash.get(objective) or {}
        fields = objective_row.get('fields', {})
        entry = {'key': objective_row.get('key') or 'Hash_%016X' % objective[1],
                 'condition': translate(lua_text(binary, fields.get(DEFINITION)))}
        # tObjectiveTriggers bytes 0..13 (which messages evaluate it) and eTriggerActivationMode (b840 §1, b864).
        triggers = inherited_field(rows_by_hash, objective, TRIGGERS)
        raw = bytes.fromhex(triggers[0]['data']) if triggers and triggers[0].get('data') else b''
        entry['triggers'] = [bool(raw[i]) if i < len(raw) else False for i in range(14)]
        mode = inherited_field(rows_by_hash, objective, ACTIVATION_MODE)
        entry['activation'] = struct.unpack('>I', bytes.fromhex(mode[0]['data'])[:4])[0] if mode and mode[0].get('data') else 0
        # PointsValid (b840): the gate on adding points to this objective, inherited from the family rows
        # (ots_*_0 take IsSequenceNewlyCompletePoints from own_the_spots); absent = retail's default false.
        found = inherited_field(rows_by_hash, objective, POINTS_VALID)
        if found and found[0].get('data'):
            # The text offset points into the bank the found row came from (family rows sit in the main bank).
            source = (binary_of or (lambda _row: None))(found[1]) or binary
            entry['points_valid'] = translate(lua_text(source, found[0]))
        failures = inherited_field(rows_by_hash, objective, FAILURES_WITH_EVENTS)
        entry['failures'] = [failure_row(rows_by_hash, ref, binary, binary_of) for ref in refs(failures[0] if failures else None)]
        # The inline FailureObjectives (b940: checked after the with-events list when none of those fired; empty on
        # tutorials, the leader distance on follow the leader, b1006 §0.3): condition and failure text per item.
        inline = inherited_field(rows_by_hash, objective, FAILURES_INLINE)
        entry['failures_inline'] = []
        if inline:
            from .objective_lua import LuaSyntaxError
            source = (binary_of or (lambda _row: None))(inline[1]) or binary
            for item in ((inline[0].get('array') or {}).get('items') or []):
                raw_item = bytes.fromhex(item)
                text_offset = struct.unpack_from('>I', raw_item, 4)[0] if len(raw_item) >= 8 else 0
                try:
                    condition = translate(lua_text(source, {'data': item}))
                except LuaSyntaxError:
                    condition = None
                text = cstring(source, text_offset).decode('utf-8') if text_offset else None
                entry['failures_inline'].append({'condition': condition, 'text': text or None})
        first = inherited_field(rows_by_hash, objective, FAILURE_FIRST)
        entry['failure_first'] = bool(first and first[0].get('data', '00')[:2] == '01')
        hints = inherited_field(rows_by_hash, objective, HINTS)
        entry['hints'] = [failure_row(rows_by_hash, ref, binary, binary_of) for ref in refs(hints[0] if hints else None)]
        out.append(entry)
    return out


def decoded_events(field, *binaries):
    """A presentation event list (`progression.presentation_events`) with its string arguments read from the first of
    `binaries` they decode in; when none fits, the event count as before (a row is never dropped over its events)."""
    from .progression import presentation_events
    for binary in binaries:
        if binary is None:
            continue
        try:
            return presentation_events(field, binary)
        except (ValueError, IndexError, struct.error):
            continue
    items = ((field or {}).get('array') or {}).get('items') or []
    if items:
        print(f'challenges: {len(items)} presentation events kept as a count (arguments not decodable)', file=sys.stderr)
        if os.environ.get('SKATE_EXPORT_EVENT_DEBUG'):
            # Where the arguments point (research aid): each record, and the bytes at each offset in every candidate.
            for item in items:
                kind, arg1, arg2, mask = struct.unpack('>IIII', bytes.fromhex(item)[:16])
                print(f'  record {item} type {kind} mask {mask}', file=sys.stderr)
                for name, offset in (('arg1', arg1), ('arg2', arg2)):
                    for i, binary in enumerate(b for b in binaries if b is not None):
                        around = binary[max(offset - 1, 0):offset + 24] if offset < len(binary) else b'(past end)'
                        print(f'    {name} {offset:#x} in binary {i} (len {len(binary):#x}): {around!r}', file=sys.stderr)
    return len(items)


def failure_row(rows_by_hash, ref, binary, binary_of=None):
    """A failure row (b940): its key, its predicate as an exported program (None when the translator cannot take it:
    the engine then never fires it) and the presentation events it plays (b943: {type, args, mask}, args from the row's
    own bank binary)."""
    from .objective_lua import translate, LuaSyntaxError
    row = rows_by_hash.get(ref) or {}
    fields = row.get('fields', {})
    source = (binary_of or (lambda _row: None))(row) or binary
    try:
        condition = translate(lua_text(source, fields.get(DEFINITION)))
    except LuaSyntaxError:
        condition = None
    events = decoded_events(fields.get(PRESENTATION_EVENTS), source, binary)
    return {'key': row.get('key') or 'Hash_%016X' % ref[1], 'condition': condition, 'presentation': events}


def inherited_field(rows_by_hash, ref, name):
    """(field, row) of the row `ref` = (class hash, key hash), else of its parent rows (same class)."""
    seen = set()
    while ref and ref not in seen:
        seen.add(ref)
        row = rows_by_hash.get(ref)
        if not row:
            return None
        if name in row.get('fields', {}):
            return row['fields'][name], row
        parent = row.get('parent')
        ref = (ref[0], key_hash(parent)) if parent else None
    return None


def _u32(value):
    return int(value['data'][:8], 16) if value and value.get('data') else None


def own_the_lots(rows, rows_by_hash, origin, global_locators, text, skipped):
    """Each own the lot row (b922): its children in disc order with their activity type, title / description ids and
    the sign-up locator; DebugOnly lots are left out. The engine sorts the children by type (screen 0x1B)."""
    names = {h: row['key'].casefold() for (c, h), row in rows_by_hash.items() if c == CHALLENGE}
    lots = []
    for (cls, key) in sorted(rows):
        if cls != CHALLENGE:
            continue
        fields, _ = chain(rows, cls, key)
        children = [names.get(k, 'Hash_%016X' % k) for c, k in refs(rows[cls, key][1].get(LOT_CHILDREN))]
        if _u32(fields.get(CHALLENGE_TYPE)) != LOT_TYPE or not children:
            continue
        if (fields.get(DEBUG_ONLY) or {}).get('data', '00')[:2] == '01':
            continue
        source, bank_binary = origin[cls, key]
        world = int((fields.get(WORLD) or {}).get('data', '0' * 16)[:16], 16)
        district = next((d for d, _ in global_locators() if hash64(d) == world), None)
        location = None
        if location_field(fields) and district:
            name = cstring(bank_binary, int(location_field(fields)['data'], 16)).decode('utf-8')
            found = global_locators().get((district, name), [])
            if len({json.dumps(m) for m in found}) == 1:
                location = dict(locator=name, matrix=found[0])
        if location is None:
            skipped.append(f'{key} (own the lot sign-up location)')
        title_id = (fields.get(TITLE) or {}).get('data') or None
        lots.append(dict(
            id=key, source=source, map=label_of(district) if district else None, district=district,
            title_id=title_id, title=text.get(title_id) if title_id else None,
            description_id=(fields.get(DESCRIPTION) or {}).get('data') or None, location=location,
            children=[dict(id=child, activity_type=_u32(chain(rows, cls, child)[0].get(ACTIVITY_TYPE)),
                           title_id=(chain(rows, cls, child)[0].get(TITLE) or {}).get('data') or None)
                      for child in children]))
    return lots


def lua_text(binary, field):
    """A tCompiledLua / tObjectiveDefinition field's text: its first u32 is the offset of the source in the bank .bin."""
    if not field or not field.get('data'):
        return ''
    return cstring(binary, struct.unpack_from('>I', bytes.fromhex(field['data']), 0)[0]).decode('utf-8')


def export(game_root, dlc_vaults, dlc_bigs, work, output, text=None, report=print):
    """Writes `output` (challenges.json) and returns its challenges."""
    from .vlt import convert, vault
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'vendor/university/tools/vanilla_map_extraction/tools'))
    from .map_volumes import district_volumes, stream_arenas
    work = Path(work)
    text = text or {}
    names = (Path(__file__).resolve().parent / 'names.txt').read_text(encoding='utf-8').splitlines()

    local, banks, hulls, frameworks = {}, [], {}, []
    for source, archive in _sources(game_root, dlc_bigs):
        entries = {e.path.replace('\\', '/').lower(): e for e in archive.entries}
        if FRAMEWORK + '.vlt' in entries and FRAMEWORK + '.bin' in entries:
            frameworks.append((archive, entries[FRAMEWORK + '.vlt'], entries[FRAMEWORK + '.bin']))
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

    def binary_of(row):
        return origin.get((row['class'], row['key'].casefold()), (None, None))[1]

    framework = {}
    for index, (archive, vlt_entry, bin_entry) in enumerate(frameworks):
        data, _ = read_vault(archive, vlt_entry, bin_entry, f'framework/{index}')
        for row in data['collections']:
            if row['class'] == PARAMS:
                framework.setdefault(key_hash(row['key']), (row['parent'], row['fields']))

    cache = {}

    def global_locators():
        from .map_starts import all_locators
        if 'records' not in cache:
            cache['records'] = all_locators(game_root, dlc_vaults)
        return cache['records']

    def volume_of(folder, instance):
        return next((v for v in district_volumes(folder, strict=False) if v['instance_id'] == '%016x' % instance), None)

    def tutorial(key, fields, bank_binary):
        """A tutorial row (b858): start and sign-up locators, the local parameters, its objective rows."""
        if key not in local:
            raise ValueError('no local data vault')
        _, archive, vlt_entry, bin_entry = local[key]
        data, binary = read_vault(archive, vlt_entry, bin_entry, 'local')
        own_row = next((r for r in data['collections'] if r['key'].casefold() == key), None)
        own = (own_row or {}).get('fields', {})
        world = int((fields.get(WORLD) or {}).get('data', '0' * 16)[:16], 16)
        district = next((d for d, _ in global_locators() if hash64(d) == world), None)
        if district is None:
            raise ValueError('no district for the row world')
        folder = work / 'hulls' / MISSIONS / key
        if key in hulls and not folder.is_dir():
            hulls[key][0].extract_entries(hulls[key][1], work / 'hulls')

        def place(name):
            found = global_locators().get((district, name), [])
            if not found and folder.is_dir():
                found = [r['matrix'] for _, _, arena in stream_arenas(folder) for r in location_records(arena) if r['locator'] == name]
            if len({json.dumps(m) for m in found}) != 1:
                raise ValueError(f'locator {name}: {len(found)} records')
            return dict(locator=name, matrix=found[0])

        location = location_field(fields)
        if not location:
            raise ValueError('no Location')
        sign_up = place(cstring(bank_binary, int(location['data'], 16)).decode('utf-8'))
        start = place(cstring(binary, int(own[START]['data'], 16)).decode('utf-8')) if START in own else sign_up
        ignore = own.get(IGNORE_REPOSITION) or fields.get(IGNORE_REPOSITION)
        radius_up = radius(fields, SIGN_UP_RADIUS)
        title_id = (fields.get(TITLE) or {}).get('data') or None
        return dict(
            id=key, family=TUTORIAL, map=label_of(district), district=district,
            title_id=title_id, title=text.get(title_id) if title_id else None,
            description_id=(fields.get(DESCRIPTION) or {}).get('data') or None,
            start=start, location=sign_up, sign_up_radius=radius_up or 4.0,
            discovery_radius=radius(fields, DISCOVERY_RADIUS) or radius_up or 4.0,
            spawn_offboard=bool(own.get(SPAWN_OFFBOARD, {}).get('data', '00')[:2] == '01'),
            persistent=own.get(PERSISTENT, {}).get('data', '00')[:2] == '01',
            ignore_initial_reposition=None if ignore is None else ignore.get('data', '00')[:2] == '01',
            presentation={name: decoded_events(own.get(field), binary, bank_binary) for name, field in PRESENTATION.items()},
            objectives=objective_rows(rows_by_hash, refs(fields.get(OBJECTIVES)), bank_binary, binary_of))

    def race(key, fields, bank_binary):
        """A race row (b979): heats with their legs (split triggers in order, the last = the finish; gates), times,
        AI path filter and start; competitors from the bank row."""
        if key not in local:
            raise ValueError('no local data vault')
        if key not in hulls:
            raise ValueError('no hull')  # race_ind_01: a leftover beside race_indu_01..03
        _, archive, vlt_entry, bin_entry = local[key]
        data, binary = read_vault(archive, vlt_entry, bin_entry, 'local')
        by_hash = {(r['class'], key_hash(r['key'])): r for r in data['collections']}
        folder = work / 'hulls' / MISSIONS / key
        if not folder.is_dir():
            hulls[key][0].extract_entries(hulls[key][1], work / 'hulls')
        world = int((fields.get(WORLD) or {}).get('data', '0' * 16)[:16], 16)
        district = next((d for d, _ in global_locators() if hash64(d) == world), None)

        def merged(ref):
            out, seen = {}, set()
            while ref and ref not in seen and ref in by_hash:
                seen.add(ref)
                row = by_hash[ref]
                for name, value in row['fields'].items():
                    out.setdefault(name, value)
                ref = (ref[0], key_hash(row['parent'])) if row.get('parent') else None
            return out

        def i32(f, name, default=None):
            value = f.get(name)
            return struct.unpack('>i', bytes.fromhex(value['data'][:8]))[0] if value and value.get('data') else default

        def trigger(raw):
            dist, name, instance = parse_trigger(raw, binary)
            volume = volume_of(folder, instance)
            if volume is None:
                raise ValueError(f'hull lacks volume {name}')
            return dict(name=name, id='%016x' % instance, volume=volume)

        def start_of(name):
            found = [r['matrix'] for _, _, arena in stream_arenas(folder) for r in location_records(arena)
                     if r['locator'] == name]
            if not found and district:
                found = global_locators().get((district.casefold(), name), [])
            if len({json.dumps(m) for m in found}) != 1:
                raise ValueError(f'start locator {name}: {len(found)} records')
            # The racers' start slots (b989 / b992): child n of the locator is competitor n's start (the player 0).
            slots = next((s for _, _, arena in stream_arenas(folder) for s in [locator_slots(arena, name)] if s), [])
            return dict(locator=name, matrix=found[0], slots=slots)

        params = merged((PARAMS, hash64(key)))
        heats = []

        def ribbon(value):
            """A tSplineBankObject: {indicator, colour} row names (Hash_<16 hex> when the row is not in the database)."""
            raw = bytes.fromhex((value or {}).get('data') or ((((value or {}).get('array') or {}).get('items') or [''])[0]) or '')
            if len(raw) < 48:
                return None
            indicator, colour = struct.unpack_from('>Q', raw, 8)[0], struct.unpack_from('>Q', raw, 32)[0]
            name = lambda h: ribbon_names.get(h, 'Hash_%016X' % h) if h else None
            return dict(indicator=name(indicator), colour=name(colour)) if indicator else None

        def flag(f, field):
            value = (f.get(field) or {}).get('data')
            return bool(int(value[:2], 16)) if value else None

        def refspec_name(value):
            raw = bytes.fromhex((value or {}).get('data') or '')
            return icon_names.get(struct.unpack_from('>Q', raw, 8)[0]) if len(raw) >= 16 else None
        for heat_ref in refs(params.get(RACE_HEATS)):
            heat = merged(heat_ref)
            if START not in heat:
                raise ValueError('heat without StartLocation')
            paths = bytes.fromhex((heat.get(AI_PATHS) or {}).get('data') or '')
            names = []
            if len(paths) >= 0x328:
                for offset in struct.unpack_from('>200I', paths, 0):
                    name = cstring(binary, offset).decode('utf-8') if offset else ''
                    if name:
                        names.append(name)
            legs = []
            for leg_ref in refs(heat.get(LEGS)):
                leg = merged(leg_ref)
                splits = [trigger(bytes.fromhex(item)) for item in ((leg.get(SPLITS) or {}).get('array') or {}).get('items') or []]
                if not splits:
                    raise ValueError('leg without split triggers')
                gates = []
                for gate_ref in refs(leg.get(GATES)):
                    gate = merged(gate_ref)
                    raw = bytes.fromhex((gate.get(GATE_VOLUME) or {}).get('data') or '')
                    _, name, instance = parse_trigger(raw, binary)
                    gates.append(dict(name=name, id='%016x' % instance, volume=volume_of(folder, instance),
                                      indicator=_u32(gate.get(GATE_INDICATOR)), ribbon=ribbon(gate.get(GATE_RIBBON)),
                                      time_bonus=i32(gate, GATE_TIME_BONUS, 0), audio=_u32(gate.get(GATE_AUDIO))))
                legs.append(dict(end_time_bonus=i32(leg, END_TIME_BONUS, 0), splits=splits, gates=gates))
            if not legs:
                raise ValueError('heat without legs')
            heats.append(dict(
                key='%016x' % heat_ref[1],
                start=start_of(cstring(binary, int(heat[START]['data'][:8], 16)).decode('utf-8')),
                time_limit=i32(heat, TIME_LIMIT), time_limit_warning=i32(heat, TIME_LIMIT_WARNING),
                killed_it_time=radius(heat, KILLED_IT_TIME),
                ai_path_index_override=i32(heat, AI_PATH_INDEX_OVERRIDE, -1),
                ai_paths=dict(names=names,
                              all_paths=struct.unpack_from('>I', paths, 0x320)[0] == 1 if len(paths) >= 0x328 else True,
                              param=struct.unpack_from('>I', paths, 0x324)[0] if len(paths) >= 0x328 else 0),
                legs=legs))
        if not heats:
            raise ValueError('no RaceHeats')

        def competitors(field):
            """tCompetitorInfo (b987): a text offset into the bank .bin naming an ai_characters row, and the kind
            (0 that character, 1 a random pick from a pool; races use 0)."""
            out = []
            for item in ((fields.get(field) or {}).get('array') or {}).get('items') or []:
                offset, kind = struct.unpack_from('>II', bytes.fromhex(item), 0)
                out.append(dict(character=cstring(bank_binary, offset).decode('utf-8'), kind=kind))
            return out

        title_id = (fields.get(TITLE) or {}).get('data') or None
        return dict(
            id=key, family=RACES, map=label_of(district) if district else None, district=district,
            title_id=title_id, title=text.get(title_id) if title_id else None,
            description_id=(fields.get(DESCRIPTION) or {}).get('data') or None,
            description_arg=int(((fields.get(DESCRIPTION_ARG) or {}).get('data') or '00')[:2], 16),
            sign_up_radius=radius(fields, SIGN_UP_RADIUS), discovery_radius=radius(fields, DISCOVERY_RADIUS),
            challenge_type=_u32(fields.get(CHALLENGE_TYPE)),
            competitors=competitors(COMPETITORS), online_competitors=competitors(ONLINE_COMPETITORS), heats=heats,
            gates=dict(skipable=flag(params, RACE_SKIPABLE), max_minimap_icons=i32(params, RACE_MAX_MINIMAP_ICONS),
                       grey_icon=refspec_name(params.get(RACE_GREY_ICON)), grey_ribbon=ribbon(params.get(RACE_GREY_RIBBON)),
                       final_ribbon=ribbon(params.get(RACE_FINAL_RIBBON)),
                       leg_finish_ribbon=ribbon(params.get(RACE_LEG_FINISH_RIBBON))))

    def contest(key, fields, bank_binary, family):
        """A contest or demo row (b1009): its rounds from the local vault; competitors from the bank row."""
        if key not in local:
            raise ValueError('no local data vault')
        _, archive, vlt_entry, bin_entry = local[key]
        data, binary = read_vault(archive, vlt_entry, bin_entry, 'local')
        by_hash = {(r['class'], key_hash(r['key'])): r for r in data['collections']}

        def merged(ref):
            out, seen = {}, set()
            while ref and ref not in seen and ref in by_hash:
                seen.add(ref)
                row = by_hash[ref]
                for name, value in row['fields'].items():
                    out.setdefault(name, value)
                ref = (ref[0], key_hash(row['parent'])) if row.get('parent') else None
            return out

        def num(f, name, kind, default=None):
            value = (f.get(name) or {}).get('data')
            if not value:
                return default
            raw = bytes.fromhex(value)
            return {'i16': lambda: struct.unpack_from('>h', raw)[0], 'i32': lambda: struct.unpack_from('>i', raw)[0],
                    'f32': lambda: round(struct.unpack_from('>f', raw)[0], 6), 'bool': lambda: raw[0] != 0}[kind]()

        def locator(f, name):
            value = (f.get(name) or {}).get('data')
            return cstring(binary, int(value[:8], 16)).decode('utf-8') if value and int(value[:8], 16) else None

        def volume_name(raw):
            try:
                return parse_trigger(raw, binary)[1]
            except (ValueError, struct.error, IndexError):
                return None

        def volume_ref(raw):
            try:
                _, name, instance = parse_trigger(raw, binary)
                return dict(name=name, instance=instance)
            except (ValueError, struct.error, IndexError):
                return None

        params = merged((PARAMS, hash64(key)))
        rounds = []
        for item in ((params.get(CONTEST_ROUNDS) or {}).get('array') or {}).get('items') or []:
            ref = (f'Hash_{item[:16]}', int(item[16:32], 16))
            r = merged(ref)
            paths = bytes.fromhex((r.get(ROUND_PATHS) or {}).get('data') or '')
            names = []
            if len(paths) >= 0x328:
                for offset in struct.unpack_from('>200I', paths, 0):
                    name = cstring(binary, offset).decode('utf-8') if offset else ''
                    if name:
                        names.append(name)
            boundary = (r.get(ROUND_BOUNDARY) or {}).get('data')
            spots = ((r.get(SPOT_VOLUMES) or {}).get('array') or {}).get('items') or []
            rounds.append(dict(
                key='%016x' % ref[1], variant=num(r, CONTEST_VARIANT, 'i32', 0), run_type=num(r, RUN_TYPE, 'i32', 0),
                round_time=num(r, ROUND_TIME, 'i16', 0), ai_adjustment=num(r, AI_SCORE_ADJUSTMENT, 'f32', 1.0),
                cumulative=num(r, CUMULATIVE, 'bool', False), cumulative_min=num(r, CUMULATIVE_MIN, 'i32', 0),
                solo_runs=num(r, SOLO_RUN_NUM, 'i16', 1), solo_ai_min=num(r, SOLO_AI_MIN, 'i16', 0),
                solo_ai_max=num(r, SOLO_AI_MAX, 'i16', 0), demo_target=num(r, DEMO_TARGET, 'i32'),
                start=locator(r, START), wait=locator(r, WAIT_LOCATION),
                boundary=volume_name(bytes.fromhex(boundary)) if boundary else None,
                spot_volumes=[n for n in (volume_name(bytes.fromhex(i)) for i in spots) if n],
                _volume_refs=dict(boundary=volume_ref(bytes.fromhex(boundary)) if boundary else None,
                                  spots=[v for v in (volume_ref(bytes.fromhex(i)) for i in spots) if v]),
                result_columns=[(r.get(c) or {}).get('data') or '' for c in RESULT_COLUMNS],
                ai_paths=dict(names=names,
                              all_paths=struct.unpack_from('>I', paths, 0x320)[0] == 1 if len(paths) >= 0x328 else True,
                              param=struct.unpack_from('>I', paths, 0x324)[0] if len(paths) >= 0x328 else 0)))
        if not rounds:
            raise ValueError('no ContestRounds')

        def competitors(field):
            out = []
            for item in ((fields.get(field) or {}).get('array') or {}).get('items') or []:
                offset, kind = struct.unpack_from('>II', bytes.fromhex(item), 0)
                out.append(dict(character=cstring(bank_binary, offset).decode('utf-8'), kind=kind))
            return out

        world = int((fields.get(WORLD) or {}).get('data', '0' * 16)[:16], 16)
        district = next((d for d, _ in global_locators() if hash64(d) == world), None)
        # The start locators sit in the contest's hull (missions/<key>), else in the district's locators.
        hull = None
        if key in hulls:
            hull = work / 'hulls' / MISSIONS / key
            if not hull.is_dir():
                hulls[key][0].extract_entries(hulls[key][1], work / 'hulls')
        for r in rounds:
            found = [rec['matrix'] for _, _, arena in stream_arenas(hull) for rec in location_records(arena)
                     if rec['locator'] == r['start']] if hull and r['start'] else []
            source = hull if found else None
            if not found and r['start']:
                # Some rows name another contest's locators (street_dtwn_01 uses street_dwtn_02's): that contest's hull.
                for other in sorted((h for h in hulls if r['start'].startswith(h + '_')), key=len, reverse=True):
                    folder = work / 'hulls' / MISSIONS / other
                    if not folder.is_dir():
                        hulls[other][0].extract_entries(hulls[other][1], work / 'hulls')
                    found = [rec['matrix'] for _, _, arena in stream_arenas(folder) for rec in location_records(arena)
                             if rec['locator'] == r['start']]
                    if found:
                        source = folder
                        break
            if not found and district and r['start']:
                found = global_locators().get((district.casefold(), r['start']), [])
            r['start_matrix'] = found[0] if len({json.dumps(m) for m in found}) == 1 else None
            # setup_round: AI.RepositionAI(id, CurRound.StartLocation, id, ...) (contest_shared @430-442): competitor n
            # stands on child n of the start locator, like the race heats (b992); the player is 0.
            r['start_slots'] = next((s for _, _, arena in stream_arenas(source) for s in [locator_slots(arena, r['start'])] if s),
                                    []) if source and r['start'] else []
            # The round boundary and spot volumes (run_round trigger enter / exit: inBoundary, inNumSpotVol), the
            # shapes from the hull the round's locators came from (as the race gates).
            refs = r.pop('_volume_refs')
            def shape(ref):
                found = volume_of(source, ref['instance']) if source and ref else None
                return dict(name=ref['name'], id='%016x' % ref['instance'], volume=found) if found else None
            r['boundary_volume'] = shape(refs['boundary'])
            r['spot_volume_shapes'] = [v for v in (shape(x) for x in refs['spots']) if v]
        # The challenge's music playlist (8248B258 reads challenge attrib 874EC914B4D34708; default 614B4D4D82510720).
        playlist = (params.get(MUSIC_PLAYLIST) or fields.get(MUSIC_PLAYLIST) or {}).get('data') or ''
        title_id = (fields.get(TITLE) or {}).get('data') or None
        return dict(
            music_playlist=playlist[16:32].upper() if len(playlist) >= 32 else '614B4D4D82510720',
            id=key, family=family, demo=family == 'demos', map=label_of(district) if district else None, district=district,
            title_id=title_id, title=text.get(title_id) if title_id else None,
            description_id=(fields.get(DESCRIPTION) or {}).get('data') or None,
            sign_up_radius=radius(fields, SIGN_UP_RADIUS), discovery_radius=radius(fields, DISCOVERY_RADIUS),
            challenge_type=_u32(fields.get(CHALLENGE_TYPE)), music_type=num(params, MUSIC_TYPE, 'i32', num(fields, MUSIC_TYPE, 'i32')),
            competitors=competitors(COMPETITORS), rounds=rounds)

    def follow(key, fields, bank_binary):
        """A follow the leader row (b875 / b1006): the local params over their framework parents, the start (with its
        slots: the player 0, the leader 1), the end volume the leader and the player race to, the objective groups."""
        if key not in local:
            raise ValueError('no local data vault')
        if key not in hulls:
            raise ValueError('no hull')
        _, archive, vlt_entry, bin_entry = local[key]
        data, binary = read_vault(archive, vlt_entry, bin_entry, 'local')
        own_row = next((r for r in data['collections'] if r['class'] == PARAMS and key_hash(r['key']) == hash64(key)), None)
        if own_row is None:
            raise ValueError('no params row')
        p, seen, parent = dict(own_row['fields']), set(), own_row['parent']
        while parent and key_hash(parent) not in seen and key_hash(parent) in framework:
            seen.add(key_hash(parent))
            parent, more = framework[key_hash(parent)]
            for name, value in more.items():
                p.setdefault(name, value)
        folder = work / 'hulls' / MISSIONS / key
        if not folder.is_dir():
            hulls[key][0].extract_entries(hulls[key][1], work / 'hulls')
        world = int((fields.get(WORLD) or {}).get('data', '0' * 16)[:16], 16)
        district = next((d for d, _ in global_locators() if hash64(d) == world), None)

        def raw(name):
            return bytes.fromhex(((p.get(name) or fields.get(name)) or {}).get('data') or '')

        def f32(name, default=None):
            b = raw(name)
            return round(struct.unpack('>f', b[:4])[0], 6) if len(b) >= 4 else default

        def flag(name, default=False):
            b = raw(name)
            return b[0] != 0 if b else default

        def number(name, default=None):
            """A float field, or an int field (whole values read as a tiny float are taken as the int)."""
            b = raw(name)
            if len(b) < 4:
                return default
            value = struct.unpack('>f', b[:4])[0]
            return struct.unpack('>i', b[:4])[0] if 0 < abs(value) < 1e-30 else round(value, 6)

        def trigger(name):
            b = raw(name)
            if not b:
                return None
            try:
                _, volume_name, instance = parse_trigger(b, binary)
            except (ValueError, struct.error, IndexError):
                return None
            shape = volume_of(folder, instance)
            return dict(name=volume_name, id='%016x' % instance, volume=shape) if shape else None

        start_raw = raw(START)
        if not start_raw:
            raise ValueError('no StartLocation')
        start_name = cstring(binary, struct.unpack('>I', start_raw[:4])[0]).decode('utf-8')
        starts = [r['matrix'] for _, _, arena in stream_arenas(folder) for r in location_records(arena) if r['locator'] == start_name]
        if not starts and district:
            starts = global_locators().get((district.casefold(), start_name), [])
        if len({json.dumps(m) for m in starts}) != 1:
            raise ValueError(f'start locator {start_name}: {len(starts)} records')
        slots = next((s for _, _, arena in stream_arenas(folder) for s in [locator_slots(arena, start_name)] if s), [])
        paths = raw(ROUND_PATHS)
        names = [n for n in (cstring(binary, o).decode('utf-8') if o else '' for o in (struct.unpack_from('>200I', paths, 0) if len(paths) >= 0x328 else ())) if n]
        competitors = []
        for item in ((fields.get(COMPETITORS) or {}).get('array') or {}).get('items') or []:
            offset, kind = struct.unpack_from('>II', bytes.fromhex(item), 0)
            competitors.append(dict(character=cstring(bank_binary, offset).decode('utf-8'), kind=kind))
        location = location_field(fields)
        sign_up = None
        if location:
            location_name = cstring(bank_binary, int(location['data'], 16)).decode('utf-8')
            places = global_locators().get((district.casefold(), location_name), []) if district else []
            if not places:
                places = [r['matrix'] for _, _, arena in stream_arenas(folder) for r in location_records(arena) if r['locator'] == location_name]
            if len({json.dumps(m) for m in places}) == 1:
                sign_up = dict(locator=location_name, matrix=places[0])
        title_id = (fields.get(TITLE) or {}).get('data') or None
        return dict(
            id=key, family=FOLLOW, map=label_of(district) if district else None, district=district,
            title_id=title_id, title=text.get(title_id) if title_id else None,
            description_id=(fields.get(DESCRIPTION) or {}).get('data') or None,
            challenge_type=_u32(fields.get(CHALLENGE_TYPE)), location=sign_up,
            sign_up_radius=radius(fields, SIGN_UP_RADIUS), discovery_radius=radius(fields, DISCOVERY_RADIUS),
            start=dict(locator=start_name, matrix=starts[0], slots=slots),
            end_volume=trigger(FTL_END_VOLUME), boundary=trigger(CHALLENGE_BOUNDARY),
            max_follow_distance=number(FTL_MAX_DISTANCE),
            countdown_delay=f32(COUNTDOWN_DELAY), prefilm_time=f32(PREFILM_TIME), total_film_time=f32(TOTAL_FILM_TIME),
            time_before_replay=f32(TIME_BEFORE_REPLAY), out_of_bounds_time=number(OUT_OF_BOUNDS_TIME),
            owned_display_time=f32(OWNED_TIME), killed_display_time=f32(KILLED_TIME),
            allow_player_bail=flag(ALLOW_PLAYER_BAIL, True), allow_objective_failure=flag(ALLOW_OBJECTIVE_FAILURE),
            allow_session_markers=flag(ALLOW_SESSION_MARKERS), reset_ai_on_bail=flag(RESET_AI_ON_BAIL, True),
            reset_ai_on_marker=flag(RESET_AI_ON_MARKER, True), reset_objectives_on_marker=flag(RESET_OBJECTIVES_ON_MARKER),
            spawn_offboard=flag(SPAWN_OFFBOARD),
            intro=decoded_events(p.get(PRESENTATION['intro']), binary, bank_binary),
            ai_paths=dict(names=names, all_paths=struct.unpack_from('>I', paths, 0x320)[0] == 1 if len(paths) >= 0x328 else True,
                          param=struct.unpack_from('>I', paths, 0x324)[0] if len(paths) >= 0x328 else 0),
            competitors=competitors,
            objectives=dict(owned=objective_programs(rows_by_hash, fields.get(OWNED), bank_binary, binary_of),
                            killed=objective_programs(rows_by_hash, fields.get(KILLED), bank_binary, binary_of)))

    class_of = lambda name: 'Hash_%016X' % hash64(name)
    ribbon_names = {hash64(k): k for (c, k) in rows if c in (class_of('ribbon_indicator'), class_of('ribbon_spline_colour'))}
    icon_names = {hash64(k): k for (c, k) in rows if c == class_of('fe_map_icons')}
    result, tutorials, races, contests, follows, skipped = [], [], [], [], [], []
    for (cls, key), (parent, _) in sorted(rows.items()):
        if cls != CHALLENGE or key.endswith('_ol'):
            continue
        fields, walked = chain(rows, cls, key)
        if TUTORIAL in walked[1:]:
            try:
                tutorials.append(tutorial(key, fields, origin[cls, key][1]))
            except (KeyError, ValueError, StopIteration) as error:
                skipped.append(f'{key} ({error})')
            continue
        contest_family = next((f for f in CONTEST_FAMILIES if f in walked[1:]), None)
        if contest_family and key not in ('street', 'vert'):
            try:
                contests.append(contest(key, fields, origin[cls, key][1], contest_family))
            except (KeyError, ValueError, StopIteration, struct.error) as error:
                skipped.append(f'{key} ({error})')
            continue
        if RACES in walked[1:]:
            try:
                races.append(race(key, fields, origin[cls, key][1]))
            except (KeyError, ValueError, StopIteration, struct.error) as error:
                skipped.append(f'{key} ({error})')
            continue
        if FOLLOW in walked[1:]:
            try:
                follows.append(follow(key, fields, origin[cls, key][1]))
            except (KeyError, ValueError, StopIteration, struct.error) as error:
                skipped.append(f'{key} ({error})')
            continue
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
            own_row = next(r for r in data['collections'] if SCORING_BOUNDARY in r['fields'])
            own = own_row['fields']
            params = parameters(own_row, framework)
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
            description_arg=int(((fields.get(DESCRIPTION_ARG) or {}).get('data') or '00')[:2], 16),
            objectives_enabled=((fields.get(OBJECTIVES_ENABLED) or {}).get('data') or '00')[:2] == '01',
            boundary=dict(name=boundary_name, id='%016x' % boundary_id, volume=boundary),
            scoring=dict(name=spot_name, id='%016x' % spot_id, volume=scoring),
            spot=dict(name=target[1], id='%016x' % target[2], volume=spot),
            start=dict(locator=start_name, matrix=starts[0]),
            location=dict(locator=location_name, matrix=places[0]),
            sign_up_radius=sign_up, discovery_radius=discovery,
            owned_points=owned, killed_points=killed,
            objectives=dict(owned=objective_programs(rows_by_hash, fields.get(OWNED), bank_binary, binary_of),
                            killed=objective_programs(rows_by_hash, fields.get(KILLED), bank_binary, binary_of)),
            challenge_type=int(fields[CHALLENGE_TYPE]['data'], 16) if CHALLENGE_TYPE in fields else None, **params))
    lots = own_the_lots(rows, rows_by_hash, origin, global_locators, text, skipped)
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({'format': FORMAT, 'version': VERSION, 'challenges': result, 'tutorials': tutorials,
                                  'own_the_lots': lots, 'races': races, 'contests': contests, 'follow_the_leaders': follows},
                                 indent=1), encoding='utf-8')
    report(f'Challenges: {len(result)} ({", ".join(FAMILIES)}) on {len({c["map"] for c in result})} maps, '
           f'{len(tutorials)} tutorials, {len(lots)} own the lots, {len(races)} races, {len(follows)} follow the leader'
           + (f'; left out: {", ".join(skipped)}' if skipped else ''))
    return result
