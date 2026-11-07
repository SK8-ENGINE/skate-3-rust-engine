"""Setup export of retail career progression (research b853, b855, b861, b887): progression groups, actions with their
trigger conditions, reward rows and each challenge's owned it / killed it rewards, as `progression.json` for the
engine's career module, plus the progression graphs (compiled Lua handlers translated to ops by progression_lua.py)."""
import json
import struct
from pathlib import Path

from .vlt import hash64

FORMAT = 'skate3rust.progression'
VERSION = 1

PROGRESSION = 'data/db/progressionbanks/main'
GROUP_CLASS = 'Hash_C04CEAAE6645A838'     # progression_group
ACTION_CLASS = 'Hash_A5970491380FF1BE'    # progression_action
REWARD_CLASS = 'Hash_9A821DB88551C7C1'    # progression_rewards
GRAPH_CLASS = 'Hash_E1A1E7B8F2EA6496'     # progression_stategraph
CHALLENGE_CLASS = 'Hash_A4F35B159D7432B5'  # challenge_global_data

TRIGGERS = 'Hash_40417FF1EFEFD7DB'       # tObservatoryTrigger array: u32 type, pad, u64 ref, u64 threshold
ACTION_GRAPH = 'Hash_7867A00C3A6F893C'   # ClassRefSpec_progression_stategraph
RECORD_FIRED = 'Hash_EC7A860A5187B15D'   # bool: keep the key in the fired list
REWARD_KIND = 'Hash_3DBC2F9C0083DF86'    # eUNLOCK_CATEGORY, inherited from parent rows
REWARD_NAME = 'Hash_8915E3FCB76529BA'    # the unlock id
REWARD_TITLE = 'Hash_AA753A536859233D'
REWARD_IMAGE = 'Hash_786255B8FF426A3D'
OWNED_REWARD = 'Hash_047ED7FFBB024AB8'   # tObservatoryProgressionReward: u32 money, u32 sales, u32 ptr, u32 count
KILLED_REWARD = 'Hash_67699C4B88712759'
ONLINE_BONUS = 'Hash_2E4824C81FDAE87C'   # u32 bonus sales (online only)
STATE_CLASS = 'Hash_EAE76EDE1246A650'     # progression_state
HANDLER_CLASS = 'Hash_B0764F22F87D1B47'   # progression_handler
GRAPH_STATES = 'Hash_3CB7BA2AFC9BDE42'    # graph -> states (class-typed RefSpec array)
STATE_HANDLERS = 'Hash_2CCC81EDC5EC2AAA'  # state -> handlers
HANDLER_BLOB = 'Hash_C3CD4B4183274819'    # (u32 size, u32 offset into the bank .bin) of a Lua 5.1 chunk

# Condition types (82C1CA48, research b855 section 2).
CONDITIONS = {0: 'board_sales', 1: 'group_done', 2: 'challenge_done', 3: 'never', 4: 'money_earned', 5: 'money'}


def _u32(field):
    return struct.unpack('>I', bytes.fromhex(field['data'][:8]))[0] if field and field.get('data') else None


def _flag(field):
    data = (field or {}).get('data') or ''
    return bool(data) and int(data, 16) != 0


def _names(rows):
    """hash -> key for every row, so references resolve to the stable string ids the engine and mods use."""
    out = {}
    for row in rows:
        key = row['key']
        out[int(key[5:], 16) if key.startswith('Hash_') else hash64(key)] = key
    return out


def _text(field, names):
    """A text field, or a hashed name field resolved through the known names."""
    if not field:
        return None
    data = field.get('data')
    if field.get('type') == 'EA::Reflection::Text':
        return data or None
    if data and len(data) == 16:
        value = int(data, 16)
        return names.get(value, 'Hash_%016X' % value)
    return data or None


def _inherited(rows_by_key, row, field):
    seen = set()
    while row is not None and row['key'] not in seen:
        seen.add(row['key'])
        if field in row['fields']:
            return row['fields'][field]
        row = rows_by_key.get((row['class'], row['parent'].casefold())) if row.get('parent') else None
    return None


def conditions(field, names):
    """The AND list of an action's trigger conditions (24-byte items)."""
    out = []
    for item in ((field or {}).get('array') or {}).get('items') or []:
        raw = bytes.fromhex(item)
        if len(raw) < 24:
            continue
        kind, _, ref, threshold = struct.unpack('>IIQQ', raw[:24])
        entry = {'type': CONDITIONS.get(kind, f'ignored_{kind}'), 'threshold': threshold}
        if kind in (1, 2):
            entry['ref'] = names.get(ref, 'Hash_%016X' % ref)
        out.append(entry)
    return out


TYPED_REF = 'Attrib::Gen::ClassRefSpec_'


def _ref_key(field):
    """The key of a single reference field: a class-typed RefSpec stores the key first (the class is the type)."""
    data = (field or {}).get('data') or ''
    return struct.unpack_from('>Q', bytes.fromhex(data), 0)[0] if len(data) >= 16 else None


def _ref_keys(field, names, class_name):
    """The keys of a class-typed RefSpec array of `class_name` rows (16-byte items, key first)."""
    if (field or {}).get('type') != TYPED_REF + class_name:
        return []
    out = []
    for item in ((field or {}).get('array') or {}).get('items') or []:
        raw = bytes.fromhex(item)
        if len(raw) >= 8:
            key = struct.unpack_from('>Q', raw, 0)[0]
            out.append(names.get(key, 'Hash_%016X' % key))
    return out


# Presentation event types by index (research b874: FNV-1 32 names from the type table 830CA8A0). The order inside
# 11..14, 37..42 and 53..57 is inferred from b874's grouped rows; no progression event uses those types.
EVENT_TYPES = (
    'FadeIn', 'FadeOut', 'Speech', 'SpeechNoWait', 'Wait', 'NIS', 'PinLocalPlayer', 'UnpinLocalPlayer',
    'TeleportLocalPlayer', 'ShowHUD', 'HideHUD', 'EnableSetSessionMarker', 'DisableSetSessionMarker',
    'EnableUseSessionMarker', 'DisableUseSessionMarker', 'ShowMiniMap', 'HideMiniMap', 'EnableHOMEffect',
    'DisableHOMEffect', 'Replay', 'ShowTutorialCalloutHUD', 'HideTutorialCalloutHUD', 'ModifiedCentralMessage',
    'CentralMessage', 'ModifiedCustomCentralMessage', 'CustomCentralMessage', 'CompleteChallenge', 'RevertChallenge',
    'GrantAchievement', 'PreloadNIS', 'TeleportLocalToBreadCrumb', 'SetDynamicHullState', 'ClearDMOsWithinVolume',
    'IncrementPointsPoolPoints', 'AddGlobalDemoTime', 'FixSkaterInAir', 'UnfixSkaterInAir', 'ShowTrickAnalyzer',
    'HideTrickAnalyzer', 'ShowPumpMeter', 'HidePumpMeter', 'ShowManualMeter', 'HideManualMeter',
    'EvaluateObjectivesWhileSimPaused', 'StopEvaluatingObjectivesWhileSimPaused', 'PauseSim', 'UnpauseSim',
    'SetSimRate', 'RestoreSimRate', 'ActivateMinimalScoringHUD', 'DeactivateMinimalScoringHUD',
    'EnableInputRestriction', 'ClearInputRestrictions', 'ShowObjectDropperUI', 'HideObjectDropperUI',
    'ClearObjectDropperInputFilters', 'AddAllObjectDropperInputFilters', 'RemoveObjectDropperInputFilter',
    'WorldScriptCommand', 'WorldScriptCommandAndWait', 'DisableObjectDropper', 'EnableObjectDropper',
    'ExecuteProgressionStategraph', 'ExecuteProgressionStategraphAndWait', 'FMV', 'SFX', 'AddIcon', 'RemoveIcon',
    'ShowMapTutorial', 'HideObjectDropperCursor', 'ForceGrindLengthDisplay', 'ForceBailTimeDisplay', 'None')
assert len(EVENT_TYPES) == 73


def _cstr(binary, offset):
    if offset >= len(binary) or (offset and binary[offset - 1] != 0):
        raise ValueError('presentation event argument is not a string start: %#x' % offset)
    end = binary.index(b'\0', offset)
    return binary[offset:end].decode('ascii') or None


def presentation_events(field, binary):
    """A state's presentation events (research b892): 16-byte records {type, arg1, arg2, mask}, the arguments string
    offsets into the bank .bin, played in array order."""
    out = []
    for item in ((field or {}).get('array') or {}).get('items') or []:
        kind, arg1, arg2, mask = struct.unpack('>IIII', bytes.fromhex(item)[:16])
        if kind >= len(EVENT_TYPES):
            raise ValueError('unknown presentation event type %d' % kind)
        out.append({'type': EVENT_TYPES[kind], 'args': [_cstr(binary, arg1), _cstr(binary, arg2)], 'mask': mask})
    return out


def _typed_keys(field, names):
    """The keys of any class-typed RefSpec array (16-byte items, key first)."""
    out = []
    for item in ((field or {}).get('array') or {}).get('items') or []:
        raw = bytes.fromhex(item)
        if len(raw) >= 8:
            key = struct.unpack_from('>Q', raw, 0)[0]
            out.append(names.get(key, 'Hash_%016X' % key))
    return out


def graphs(rows, binary):
    """Every progression graph as {initial, states: {state: {events, on: {message: ops}}}}: each handler's compiled
    Lua chunk translated to ops (progression_lua.py; an unknown call or construct fails the export) and its presentation
    events decoded (b892)."""
    from .progression_lua import translate_chunk
    names = _names(rows)
    by_key = {(r['class'], r['key']): r for r in rows}
    out = {}
    for row in rows:
        if row['class'] != GRAPH_CLASS:
            continue
        states = {}
        for state_key in _typed_keys(row['fields'].get(GRAPH_STATES), names):
            state = by_key.get((STATE_CLASS, state_key))
            if state is None:
                continue
            name = _text(state['fields'].get('Name'), names) or state_key
            on = {}
            for handler_key in _typed_keys(state['fields'].get(STATE_HANDLERS), names):
                handler = by_key.get((HANDLER_CLASS, handler_key))
                blob = (handler or {}).get('fields', {}).get(HANDLER_BLOB)
                if not blob or not blob.get('data'):
                    continue
                size, offset = struct.unpack_from('>II', bytes.fromhex(blob['data']), 0)
                message = _text(handler['fields'].get('Name'), names) or handler_key
                on[message] = translate_chunk(binary[offset:offset + size])
            events = presentation_events(state['fields'].get('Events'), binary)
            states[name] = {'events': events, 'on': on}
        if states:
            out[row['key']] = {'initial': 'initial', 'states': states}
    return out


def progression(rows, known=None):
    """Groups, actions and rewards from the converted progression bank rows. `known`: hash -> name of rows in other
    banks the references point at (the challenge keys live in the challenge bank)."""
    names = dict(known or {})
    names.update(_names(rows))
    by_key = {(r['class'], r['key'].casefold()): r for r in rows}
    groups, actions, rewards = {}, {}, {}
    for row in rows:
        cls, fields = row['class'], row['fields']
        if cls == GROUP_CLASS:
            # The member list is the class-typed challenges RefSpec array; found by its type, not a field name.
            members = []
            for value in fields.values():
                members.extend(_ref_keys(value, names, 'challenges'))
            groups[row['key']] = members
        elif cls == ACTION_CLASS:
            graph_key = _ref_key(fields.get(ACTION_GRAPH))
            actions[row['key']] = {
                'parent': row.get('parent') or None,
                'conditions': conditions(fields.get(TRIGGERS), names),
                'graph': names.get(graph_key, 'Hash_%016X' % graph_key) if graph_key else None,
                'record_fired': _flag(fields.get(RECORD_FIRED)),
            }
        elif cls == REWARD_CLASS:
            rewards[row['key']] = {
                'kind': _u32(_inherited(by_key, row, REWARD_KIND)),
                'name': _text(fields.get(REWARD_NAME), names),
                'title': _text(fields.get(REWARD_TITLE), names),
                'image': _text(fields.get(REWARD_IMAGE), names),
            }
    return {'groups': groups, 'actions': actions, 'rewards': rewards}


def challenge_rewards(fields, binary, names):
    """A challenge row's owned it / killed it rewards: money, board sales and reward ids (827CDDD8 pays them)."""
    out = {}
    for label, key in (('owned', OWNED_REWARD), ('killed', KILLED_REWARD)):
        field = fields.get(key)
        if not field or not field.get('data') or len(field['data']) < 32:
            continue
        money, sales, ptr, count = struct.unpack('>IIII', bytes.fromhex(field['data'][:32]))
        ids = []
        for i in range(count):
            at = ptr + i * 24
            if 0 < ptr and at + 16 <= len(binary):
                key = struct.unpack_from('>Q', binary, at + 8)[0]
                ids.append(names.get(key, 'Hash_%016X' % key))
        out[label] = {'money': money, 'sales': sales, 'rewards': ids}
    bonus = _u32(fields.get(ONLINE_BONUS))
    if bonus:
        out['online_bonus_sales'] = bonus
    return out


KILL_RACE_TYPE = 0x13  # eChallengeTypes value that is killable without a killed it list (b884 / b886)


def killable(fields):
    """A challenge has a killed it goal: a non-empty ObjectivesKilledIt list (inherited) or the race type."""
    from .challenges import KILLED, CHALLENGE_TYPE
    if ((fields.get(KILLED) or {}).get('array') or {}).get('items'):
        return True
    kind = (fields.get(CHALLENGE_TYPE) or {}).get('data')
    return bool(kind) and int(kind, 16) == KILL_RACE_TYPE


def export(game_root, dlc_bigs, work, output, report=print):
    """Writes `output` (progression.json): the progression bank's groups / actions / rewards and every challenge's
    owned it / killed it rewards (challenge banks, family fields inherited)."""
    from .challenges import chain
    from .teleporters import _sources, BANKS
    from .vlt import convert, vault
    work = Path(work)
    names_txt = (Path(__file__).resolve().parent / 'names.txt').read_text(encoding='utf-8').splitlines()
    schema = work / 'schema'
    if not (schema / 'data/db/skaterschema.vlt').is_file():
        disc = next(a for _, a in _sources(game_root, ()))
        disc.extract_entries([e for e in disc.entries
                              if Path(e.path).name.lower() in {'skaterschema.bin', 'skaterschema.vlt'}], schema)

    def read(archive, vlt_entry, bin_entry, folder):
        stem = work / folder / Path(vlt_entry.path.replace('\\', '/')).stem
        stem.parent.mkdir(parents=True, exist_ok=True)
        for entry, suffix in ((vlt_entry, '.vlt'), (bin_entry, '.bin')):
            stem.with_suffix(suffix).write_bytes(archive.read(entry))
        return convert(schema / 'data/db/skaterschema', stem, names_txt)['collections'], vault(stem)[1]

    progression_rows, bank_rows, binaries = None, {}, {}
    for index, (_, archive) in enumerate(_sources(game_root, dlc_bigs)):
        entries = {e.path.replace('\\', '/').lower(): e for e in archive.entries}
        if progression_rows is None and PROGRESSION + '.vlt' in entries and PROGRESSION + '.bin' in entries:
            progression_rows, progression_binary = read(archive, entries[PROGRESSION + '.vlt'], entries[PROGRESSION + '.bin'],
                                                        'progression')
        for path, entry in entries.items():
            if path.startswith(BANKS) and path.endswith('.vlt') and path[:-4] + '.bin' in entries:
                rows, binary = read(archive, entry, entries[path[:-4] + '.bin'], f'banks/{index}-{len(binaries)}')
                for row in rows:
                    key = (row['class'], row['key'].casefold())
                    if key not in bank_rows:
                        bank_rows[key] = (row['parent'], row['fields'])
                        binaries[key] = binary
    if progression_rows is None:
        raise ValueError('no progression bank')
    known = {}
    for (cls, key), _ in bank_rows.items():
        if cls == CHALLENGE_CLASS:
            known[int(key[5:], 16) if key.startswith('hash_') else hash64(key)] = key
    data = progression(progression_rows, known)
    known.update(_names(progression_rows))
    rewards, kills = {}, []
    for (cls, key) in bank_rows:
        if cls != CHALLENGE_CLASS:
            continue
        fields, _ = chain(bank_rows, cls, key)
        paid = challenge_rewards(fields, binaries[cls, key], known)
        if paid:
            rewards[key] = paid
        if killable(fields):
            kills.append(key)
    data['challenge_rewards'] = rewards
    data['killable'] = sorted(kills)
    data['career'] = {'milestone_parent': 'milestone_actions', 'crossbar_gates': ['film_skpk_univ_01', 'photo_univ_05']}
    data['graphs'] = graphs(progression_rows, progression_binary)
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({'format': FORMAT, 'version': VERSION, **data}, indent=1), encoding='utf-8')
    report(f"Progression: {len(data['groups'])} groups, {len(data['actions'])} actions, {len(data['rewards'])} rewards, "
           f"{len(rewards)} challenges with rewards, {len(data['graphs'])} graphs")
    return data
