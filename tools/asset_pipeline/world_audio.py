"""World sound sources' data for the native runtime (crates/skate-audio/src/world): the traffic and
pedestrian banks, their vault tuning, and the streamed speech index.

- `WORLD_BANKS`: the AEMS banks retail loads when the living world starts (the eight engine banks,
  horn, skid, car alarms, ped footsteps, tazer); setup decodes them like every other bank.
- `world_tuning(collections, record_names)`: `aud_traffic_engine` records (by name) and the ped
  footstep fields (`skate_audio::world::peds::PedFootstepTuning`).
- `speech_index(archive)`: per clip of `livingworldspeech.big`: event, voice, line, its `.hdr` id and
  take-history length, and every take's offset / size / sample count (the `.sth` rows); plus the
  speech library's event rules parsed from `<prefix>_Events.evt` (`parse_evt`: event → records →
  clip ids, `skate_audio::world::speech_rules`); ~1 MB of JSON, no audio.
- `speech_tuning(collections)`: the speech manager's per-event tuning (`Sk8::Audio::tSpeechTuning`
  and the not-follow lists) per speech bank; part of `world_tuning`.
- `decode_speech(...)`: on by default (`SKATE_SETUP_SPEECH=0` skips it): each take of the chosen events decoded to a
  mono PCM16 WAV at its own 36 kHz (`speech/livingworld/<clip>/<take>.wav`). All free-roam events are
  ~9 h of audio (~2.4 GB), so the default setup leaves it out. The main cast's (`maincastspeech.big`)
  and the announcer's (`announcerspeech.big`, `ANNOUNCER_EVENTS`) indexes and decodes work the same.

Reading of the disc's own data at setup time; nothing from the game is committed. Formats and the
mechanism: docs/hails-additions/audio-specs/world-speech.md, world-traffic-audio.md, world-ped-audio.md.
"""
from __future__ import annotations

import io
import os
import re
import struct
from pathlib import Path

WORLD_BANKS = (
    'C00_heavy01.abk', 'C01_family01.abk', 'C03_sports01.abk', 'C04_taxi01.abk', 'C05_truck01.abk',
    'C06_sports02.abk', 'C07_family02.abk', 'C08_family03.abk', 'Traffic_Horn.abk', 'Traffic_Skid.abk',
    'car_alarms.abk', 'fstep_livingworld.abk', 'Tazer.abk', 'CellPhone_Rings.bnk',
    # The dynamic props' rolling / sliding loops (skate_audio::world::props::LOOP_BANKS, doc 27 "Prop sounds").
    'Dynamics_Objects_Roll.abk', 'Dynamics_Objects_Slide_A.abk', 'Dynamics_Objects_Slide_B.abk',
)
# The Splice (SPLC) banks among them: their patch trees go to the native Splice player
# (`skate_audio::world::peds::PedSpeech`'s phone ring, retail's Splice table index 6).
WORLD_SPLICE_BANKS = ('CellPhone_Rings.bnk',)

# The pushed props' sound banks (doc 27 "Prop sounds", research b630 / b632): each prop SFX record (class B05F...)
# names its main bank (Text at layout 0x48) and, through its RefSpec at 0x18, a one-shot record (class 74F2...)
# whose Text at offset 0 names a Splice bank; the start loops live in DMO_Starts.
PROP_RECORD_CLASS, PROP_MAIN_BANK = 'Hash_B05F7C74C63ACFB8', 'Hash_BE88128A30BE926E'
PROP_ONESHOT_CLASS, PROP_ONESHOT_BANK = 'Hash_74F2B4132A1D39EA', 'Hash_A09DA314AE53C56D'
PROP_START_BANK = 'DMO_Starts.abk'


def prop_banks(collections: list[dict]) -> tuple[set[str], set[str]]:
    """(AEMS banks, Splice banks) the prop records name, read from every converted collection (DLC ones too)."""
    abks, bnks = {PROP_START_BANK}, set()
    for c in collections:
        field = {PROP_RECORD_CLASS: PROP_MAIN_BANK, PROP_ONESHOT_CLASS: PROP_ONESHOT_BANK}.get(c['class'])
        name = (c['fields'].get(field) or {}).get('data', '').strip() if field else ''
        if name:
            (bnks if name.lower().endswith('.bnk') else abks).add(Path(name).name)
    return abks, bnks


ENGINE_CLASS = 'Hash_259095163B974174'  # aud_traffic_engine
ENGINE_FIELDS = {
    'Hash_10C7F64B3253B21F': ('idle_rpm', 'f32'),
    'Hash_DD02885FAFA71D6D': ('max_rpm', 'f32'),
    'Hash_C436B6BC22BC023C': ('patch', 'i16'),
    'Hash_2C1586C6D46B89DF': ('wobble_limit', 'f32'),
    'Hash_D048F5E809B070C0': ('wobble_rate', 'f32'),
    'Hash_E6B3BD54DF5AC0A5': ('rise', 'f32'),
    'Hash_7EA0A89887B3746C': ('fall', 'f32'),
    'Hash_7FD84F2C9C374F46': ('slew', 'f32'),
    'Hash_E67C4A17326C555D': ('gear_speed', 'f32'),
    'Hash_07CE76F8BE0066C1': ('gears', 'i16'),
    'Hash_763DB0A168A49E93': ('rear_bias', 'i32'),
}
OFFBOARD = ('Hash_C1831BDB6CB1B1EA', 'Hash_1ABD2984D7248589')
CLOTHING = ('Hash_A867FBE3454326FF', 'default')
EQ_HOLDER = ('Hash_42AFE160E647167C', 'default')
PED_CURVE = 'Hash_90B47430C4ED2CCC'
PED_SPEEDS = 'Hash_E12AF885D3C3A168'
PED_STEP_IDS = ('Hash_6B61C043E53C44CB', 'Hash_EC3399A49055DD8D', 'Hash_9D6D2863CFE908C4')
PED_TAIL = 'Hash_62A2E64238934734'
PED_EQ = 'Hash_A9023782094771B5'
# SFXObj_PedBodyFall (recomp sub_824F0AB8): the Skate_Collisions containers by BodyFallType 8 / 9 / other,
# and the eEQChain field it resolves (absent from the shipped database: the lookup's default 0).
BODY_FALL = ('Hash_923CCB46EF5BF5BA', 'Hash_DFEFC9212E0CBD2C')
BODY_FALL_IDS = ('Hash_552899F3BF9927CC', 'Hash_A3E8A9381F4222E1', 'Hash_C0FFD1E535F218E2')
BODY_FALL_EQ = 'Hash_B4C4F86A53963BA2'
# PedestrianSpeech's phone ring for speech value 49 (sub_824D9C70): CellPhone_Rings container.
RING = ('Hash_C1831BDB6CB1B1EA', 'cellphone')
RING_ID = 'Hash_031EFDF991638985'
# The speech stream voice's PEAK filter by the speaker's azimuth (PedestrianSpeech update sub_824D9370):
# three Sk8::PointNegGraphData8 curves of the speech record (holder *(0x830CFDA4)+44).
SPEECH_RECORD = ('Hash_B29C3B2C13D96482', 'default')
SPEECH_PEAK = {'freq': 'Hash_2C166907CF51DB88', 'gain': 'Hash_EA2C18D9CE5CBA3A', 'q': 'Hash_CF8679F540B82B2B'}
# The speech echo delay: the camera-distance factor and the refresh in console frames (`sub_824D9370`).
SPEECH_ECHO = {'delay_factor': ('Hash_BF48032DB145C5B4', 'f32'), 'delay_frames': ('Hash_510BAFA32A76B340', 'i32')}
# The announcer (skate_audio::world::announcer): an NPC skater's crash asks for `480_slam_pro` within this camera
# distance (sub_824DB688), and the announcer stream's level multiplier per console language group and challenge byte
# (sub_824A8250: three entries each, by the line's speaker 35 / 36 / 31). English uses `other`.
ANNOUNCER_CRASH = 'Hash_887C1D3324B12C4A'
ANNOUNCER_LEVEL = {'french': 'Hash_CCC18F677B896049', 'french_challenge': 'Hash_CAD1FAC38891DA18',
                   'german': 'Hash_7A9D965C3AE7BB73', 'german_challenge': 'Hash_E0F263477B891647',
                   'other': 'Hash_49AE841BE63F9EB7', 'other_challenge': 'Hash_60E0221FD3D6F04B'}
# The state graph that holds a tazer zap (TazeEntity: TazerCycTime).
TAZE_GRAPH = 'data/state/livingworldentities/pedestrian/aigraph/pedestrian_wanttotaze.xml'


def _graph8(field) -> dict | None:
    """A Sk8::PointNegGraphData8 field: 16 header bytes, then 8 x and 8 y floats."""
    if not field:
        return None
    raw = bytes.fromhex(''.join(field['data'].split()))
    if len(raw) < 80:
        return None
    floats = struct.unpack('>16f', raw[16:80])
    return {'x': [round(v, 6) for v in floats[:8]], 'y': [round(v, 6) for v in floats[8:]]}


def tazer_seconds(roots) -> float | None:
    """The TazeEntity state's TazerCycTime (s) from the ped state graph, in the first of `roots` (the
    extracted stock data, the disc) that has it; None when none does."""
    for root in roots or ():
        path = Path(root)/TAZE_GRAPH
        if path.is_file():
            text = path.read_text(encoding='utf-8', errors='replace')
            m = re.search(r'timerName="TazerCycTime"\s+length="([0-9.]+)"', text)
            return float(m.group(1)) if m else None
    return None

# The speech events a free-roam ped can say (audio-specs/world-speech.md): reactions, chases, conversations, phone
# calls, bums, the player-action comments.
FREE_ROAM_EVENTS = (101, 102, 104, 105, 108, 109, 110, 201, 202, 203, 204, 205, 206, 207, 314, 315, 316, 320,
                    330, 331, 335, 336, 338, 339, 400, 497, 501, 603, 604, 605, 606, 607, 609, 611, 805, 806, 807,
                    1901, 4402, 4405)
# The main cast's (pros' and the special cast's) free-roam events (`maincastspeech.big`, the speech manager's bank 0):
# the skater speech messages and reactions the port sends (grunt 201 / impact 202, crash 906, pos 101, slam 104, collide
# 130, pro-on-pro 150 / 151, gestures 338, hit reactions 344 / 335 / 336, warn 500 / 501, greet 601, race 903 / 913,
# bored 1014). ~3.0 h, ~0.95 GB as 44.1 kHz PCM16; decoded with the living world's (on unless `SKATE_SETUP_SPEECH=0`).
MAIN_CAST_EVENTS = (101, 104, 130, 150, 151, 201, 202, 335, 336, 338, 344, 500, 501, 601, 903, 906, 913, 1014)
# The announcer's events an engine sender can request (`announcerspeech.big`, speech bank 3): the NPC pro's crash
# (480_slam_pro, 571 s, ~41 MB as 36 kHz PCM16). Free skate never plays it (no challenge announcer); the other 62
# events belong to the challenge modes. Decoded with the living world's (on unless `SKATE_SETUP_SPEECH=0`).
ANNOUNCER_EVENTS = (480,)


def _word(data: str, kind: str):
    raw = bytes.fromhex(''.join(data.split()))
    if kind == 'f32':
        return round(struct.unpack('>f', raw[:4])[0], 6)
    if kind == 'i16':
        return struct.unpack('>h', raw[:2])[0]
    return struct.unpack('>i', raw[:4])[0]


def world_tuning(collections: list[dict], record_names: list[str] | None = None, state_roots=None) -> dict:
    """{'traffic_engine': {name: {...}}, 'ped_footsteps': {...}, 'ped_objects': {...}, 'speech_voice': {...}, …}
    from the converted collections (and the disc's ped state graph for the tazer hold)."""
    from .audio_formats import name_id
    names = {f'Hash_{name_id(n):016X}': n for n in (record_names or [])}
    by_class: dict[str, dict] = {}
    for c in collections:
        by_class.setdefault(c['class'], {})[c['key']] = c

    def resolve(cls: str, key: str, field: str):
        records, seen = by_class.get(cls, {}), 0
        while key in records and seen < 32:
            if field in records[key]['fields']:
                return records[key]['fields'][field]
            key, seen = records[key].get('parent', ''), seen + 1
        return None

    engines = {}
    for key in by_class.get(ENGINE_CLASS, {}):
        record = {}
        for field, (name, kind) in ENGINE_FIELDS.items():
            f = resolve(ENGINE_CLASS, key, field)
            if f is not None:
                record[name] = _word(f['data'], kind)
        engines[names.get(key, key)] = record
    out: dict = {'traffic_engine': engines}
    peds: dict = {}
    curve = resolve(*OFFBOARD, PED_CURVE)
    if curve:
        raw = bytes.fromhex(''.join(curve['data'].split()))
        if len(raw) >= 16 + 128:
            floats = struct.unpack('>32f', raw[16:16 + 128])
            peds['speed_curve_x'] = [round(v, 6) for v in floats[:16]]
            peds['speed_curve_y'] = [round(v, 6) for v in floats[16:]]
    speeds = resolve(*CLOTHING, PED_SPEEDS)
    if speeds and 'array' in speeds:
        peds['speeds'] = [_word(item, 'f32') for item in speeds['array']['items']]
    ids = [resolve(*OFFBOARD, f) for f in PED_STEP_IDS]
    if all(ids):
        peds['step_ids'] = [_word(f['data'], 'i32') for f in ids]
    tail = resolve(*OFFBOARD, PED_TAIL)
    if tail and 'array' in tail:
        peds['tail'] = [_word(item, 'i32') for item in tail['array']['items']]
    eq = resolve(*EQ_HOLDER, PED_EQ)
    if eq:
        peds['eq_chain'] = _word(eq['data'], 'i32')
    out['ped_footsteps'] = peds
    # The ped one-shot objects (skate_audio::world::peds::PedObjectTuning; unset fields keep its retail defaults).
    objects: dict = {}
    ids = [resolve(*BODY_FALL, f) for f in BODY_FALL_IDS]
    if all(ids):
        objects['body_fall_ids'] = [_word(f['data'], 'i32') for f in ids]
    eq = resolve(*EQ_HOLDER, BODY_FALL_EQ)
    objects['body_fall_eq'] = _word(eq['data'], 'i32') if eq else 0
    ring = resolve(*RING, RING_ID)
    if ring:
        objects['ring_id'] = _word(ring['data'], 'i32')
    seconds = tazer_seconds(state_roots)
    if seconds is not None:
        objects['tazer_seconds'] = seconds
    out['ped_objects'] = objects
    voice = {}
    for name, field in SPEECH_PEAK.items():
        curve = _graph8(resolve(*SPEECH_RECORD, field))
        if curve:
            voice[f'peak_{name}'] = curve
    for name, (field, kind) in SPEECH_ECHO.items():
        f = resolve(*SPEECH_RECORD, field)
        if f is not None:
            voice[name] = _word(f['data'], kind)
    crash = resolve(*SPEECH_RECORD, ANNOUNCER_CRASH)
    if crash is not None:
        voice['announcer_crash_m'] = _word(crash['data'], 'f32')
    level = {}
    for name, field in ANNOUNCER_LEVEL.items():
        f = resolve(*SPEECH_RECORD, field)
        if f and 'array' in f:
            level[name] = [_word(item, 'f32') for item in f['array']['items']]
    if level:
        voice['announcer_level'] = level
    out['speech_voice'] = voice
    out['speech_tuning'] = speech_tuning(collections)
    out['ped_models'] = ped_models(collections)
    out['traffic_models'] = traffic_models(collections, names)
    out['vehicle_alarm'] = vehicle_alarm(collections)
    return out


# The per-model audio fields of living-world peds (`aud_characteristics`, read by the ped audio state's
# activation, recomp sub_824F91B0; audio-specs/world-audio-hookin-spec.md §7.3 G2). Keyed by the model's
# speech voice id (the record's `Character` field = the clip names' voice).
PED_CLASS = 'aud_characteristics'
PED_MODEL_FIELDS = {
    'Hash_EF9605D206F68DBD': ('voice', 'i32'),        # Character (S+84)
    'Hash_6BD295C16B243F93': ('variant', 'i32'),      # SPCH1Type_CharID: the voice variant bit (S+88)
    'Hash_492964E71634DA6D': ('kind', 'i32'),         # the speaker type bit (S+96; 64 = security)
    'Hash_68BB61E508841729': ('gender', 'i32'),       # S+124: 1 female, 2 male
    'Hash_871BDC669F2B1844': ('shoe_class', 'i32'),   # S+132
    'Hash_A27215A909135B62': ('far', 'f32'),          # S+156: the far line threshold (m)
    'Hash_2087A3290483BB4F': ('pitch', 'f32'),        # the per-voice float (S+152): the speech stream's gain / sends ×
    'Hash_6F2933E977CF40DD': ('cast_bit', 'i32'),     # the main-cast speaker bit (skater speech record +84, word 1)
    'Hash_14FD437D190677C8': ('cast_word', 'i32'),    # the special cast's bit (+88, word 2: characters 30-38)
    'Hash_D6EA428C2B43E23A': ('cast_word2', 'i32'),   # the second word another skater's lines read (150 / 151)
    'Hash_6F9C8A27E4CD37DC': ('announcer_id', 'i32'), # SPCH3Type_char_ID_Ann: the announcer characters 35 / 36 (1 / 2)
    'Hash_14B23B4527AF919E': ('announcer_pro', 'i32'),  # SPCH3Type_pro_id_ANN: the word 480_slam_pro names a pro by
}


def _resolver(by_class: dict, cls: str):
    records = by_class.get(cls, {})

    def resolve(key: str, field: str):
        seen = 0
        while key in records and seen < 32:
            if field in records[key]['fields']:
                return records[key]['fields'][field]
            key, seen = records[key].get('parent', ''), seen + 1
        return None
    return records, resolve


def ped_models(collections: list[dict]) -> dict:
    """{voice id: {variant, kind, gender, shoe_class, far, pitch, record}} for every aud_characteristics
    record that names its own voice (abstract parents keep the default `Character` 104 and are left out)."""
    from .audio_formats import name_id
    by_class: dict[str, dict] = {}
    for c in collections:
        by_class.setdefault(c['class'], {})[c['key']] = c
    records, resolve = _resolver(by_class, f'Hash_{name_id(PED_CLASS):016X}')
    out: dict = {}
    for key in records:
        model = {}
        for field, (name, kind) in PED_MODEL_FIELDS.items():
            f = resolve(key, field)
            if f is not None:
                model[name] = _word(f['data'], kind)
        voice = model.pop('voice', None)
        if voice is None or voice in (0, 104) or str(voice) in out:
            continue
        model['record'] = key
        out[str(voice)] = model
    return out


# Traffic model -> engine record (static attribute chain, spec §7.3 G1): livingworld_entities field
# 92A043B4A11F1A2A -> livingworld_vehicle_characteristics record -> field BA2DDD830C731EE4 -> aud_traffic_engine.
VEHICLE_SPEC_FIELD = 'Hash_92A043B4A11F1A2A'
ENGINE_REF_FIELD = 'Hash_BA2DDD830C731EE4'


def traffic_models(collections: list[dict], engine_names: dict) -> dict:
    """{entity name: engine record name} for every living-world entity with a vehicle spec (after
    inheritance). `engine_names` maps the engine record keys to names (as `world_tuning`)."""
    from .audio_formats import name_id
    by_class: dict[str, dict] = {}
    for c in collections:
        by_class.setdefault(c['class'], {})[c['key']] = c
    entities, resolve_entity = _resolver(by_class, f'Hash_{name_id("livingworld_entities"):016X}')
    _, resolve_spec = _resolver(by_class, f'Hash_{name_id("livingworld_vehicle_characteristics"):016X}')
    engine_class = f'Hash_{name_id("aud_traffic_engine"):016X}'
    engines = by_class.get(engine_class, {})

    def key_of(ref: dict, records: dict):
        raw = ''.join(ref['data'].split())
        if len(raw) < 32:
            return None
        h = f'Hash_{raw[16:32].upper()}'
        if h in records:
            return h
        # The converter names records whose names it knows ('default', ...): match by hash.
        return next((k for k in records if not k.startswith('Hash_') and f'Hash_{name_id(k):016X}' == h), None)

    specs = by_class.get(f'Hash_{name_id("livingworld_vehicle_characteristics"):016X}', {})
    out: dict = {}
    for key in entities:
        ref = resolve_entity(key, VEHICLE_SPEC_FIELD)
        if ref is None:
            continue
        spec = key_of(ref, specs)
        engine_ref = resolve_spec(spec, ENGINE_REF_FIELD) if spec else None
        engine = key_of(engine_ref, engines) if engine_ref else None
        if engine is None:
            continue
        out[key] = engine_names.get(engine, engine)
    return out


# The car alarm trigger (livingworld_vehicle_characteristics; read by the vehicle's collision callback, recomp
# sub_82C3C150, and the StayingParked condition StopAlarming sub_82C3A4D0; audio-specs/world-traffic-audio.md
# "Car alarm trigger"). No shipped vehicle spec overrides either field (all inherit `default`).
VEHICLE_ALARM_FIELDS = {
    'Hash_543475921FD9E04A': 'min_impact',  # a parked car's alarm starts when the contact vector's length exceeds this
    'Hash_E199FC7CEA222809': 'seconds',     # ... and stops when its timer (reset by every such contact) passes this
}


def vehicle_alarm(collections: list[dict]) -> dict:
    """{'min_impact': f, 'seconds': f} from the `default` vehicle spec, plus {'specs': {spec: {field: f}}} for the
    specs whose resolved value differs (none in retail). Missing records: {} (the engine keeps its retail defaults)."""
    from .audio_formats import name_id
    by_class: dict[str, dict] = {}
    for c in collections:
        by_class.setdefault(c['class'], {})[c['key']] = c
    cls = f'Hash_{name_id("livingworld_vehicle_characteristics"):016X}'
    records, resolve = _resolver(by_class, cls)
    default = next((k for k in records if k == 'default' or k == f'Hash_{name_id("default"):016X}'), None)

    def values(key: str) -> dict:
        out = {}
        for field, name in VEHICLE_ALARM_FIELDS.items():
            f = resolve(key, field)
            if f is not None:
                out[name] = _word(f['data'], 'f32')
        return out

    out = values(default) if default else {}
    specs = {}
    for key in records:
        if key == default:
            continue
        diff = {k: v for k, v in values(key).items() if out.get(k) != v}
        if diff:
            specs[key] = diff
    if specs:
        out['specs'] = specs
    return out


# The speech manager's event tuning (vault class read by recomp sub_824ABA18 / sub_824A75F0 /
# sub_824A8C78; audio-specs/world-speech.md, "Mechanism", "The request").
SPEECH_CLASS = 'Hash_9C1F48F5D637E275'
SPEECH_TUNING = 'Hash_D675AF88AC03844D'      # Sk8::Audio::tSpeechTuning (64 bytes)
SPEECH_CHALLENGES = 'Hash_D4332E21D03D7541'  # Sk8::Challenge::eChallengeTypes[]: no speech during these
SPEECH_EVENT_TYPE = re.compile(r'^SPCHType_(\d)_EventID$')
SPEECH_NOT_FOLLOW = re.compile(r'^Sk8::Audio::t(LW|MC|CM|AN)NotFollow$')


def _tuning_struct(raw: bytes) -> dict:
    """The tSpeechTuning fields the manager reads (offsets in the names' comments)."""
    f = lambda o: round(struct.unpack_from('>f', raw, o)[0], 6)  # noqa: E731
    return {
        'unknown_0': f(0), 'unknown_4': f(4),
        'gap': f(8),                       # +8: s since this speaker last spoke (any event)
        'flags_12': list(raw[12:16]),      # +12..15 bytes; +13 / +14 = interrupt rules (sub_824A73F0)
        'priority': struct.unpack_from('>i', raw, 16)[0],  # +16
        'probability': f(20),              # +20: percent
        'repeat': f(24),                   # +24: s since this speaker last said this event
        'unknown_28': f(28),
        'min_player_kmh': f(32),           # +32: the player's speed × 3.6 must reach this
        'max_player_kmh': f(36),           # +36: … and stay at or below this
        'timer_40': f(40), 'timer_44': f(44),
        'flags_48': list(raw[48:52]),      # +49 / +50 / +51: tested against game flags
        # +52 / +56: the main cast's repeat time for speaker slots 31 / 30 (models 31 skate_coach and
        # 30), in place of +24 (sub_824AC560)
        'repeat_speaker_31': f(52), 'repeat_speaker_30': f(56),
        'zombie': raw[60] != 0,            # +60: allowed while zombie mode is on
    }


def speech_tuning(collections: list[dict]) -> dict:
    """{bank: {event id: tuning}}: bank 0..3 from the record's `SPCHType_<n>_EventID` field (1 = the
    living world). Records without their own tSpeechTuning inherit the parent's (`default`)."""
    records = {c['key']: c for c in collections if c['class'] == SPEECH_CLASS}

    def field(key: str, name: str, seen: int = 0):
        record = records.get(key)
        if record is None or seen > 32:
            return None
        if name in record['fields']:
            return record['fields'][name]
        return field(record.get('parent', ''), name, seen + 1)

    out: dict = {}
    for key, record in records.items():
        bank = event = None
        not_follow = []
        for value in record['fields'].values():
            m = SPEECH_EVENT_TYPE.match(value['type'])
            if m:
                bank, event = int(m.group(1)), _word(value['data'], 'i32')
            if SPEECH_NOT_FOLLOW.match(value['type']):
                for item in value.get('array', {}).get('items', []):
                    raw = bytes.fromhex(item)
                    not_follow.append([struct.unpack('>i', raw[:4])[0], round(struct.unpack('>f', raw[4:8])[0], 6)])
        if bank is None:
            continue
        tuning = field(key, SPEECH_TUNING)
        entry = _tuning_struct(bytes.fromhex(tuning['data'])) if tuning else {}
        entry['not_follow'] = not_follow
        challenges = field(key, SPEECH_CHALLENGES)
        entry['challenges'] = [_word(i, 'i32') for i in (challenges or {}).get('array', {}).get('items', [])]
        out.setdefault(str(bank), {})[str(event)] = entry
    return out


CLIP = re.compile(r'^(\d+)_(\d+)_(?:([a-z]+\d)_)?(.+)\.dat$')


def parse_clip_name(name: str):
    """'501_59_busm1_Warn_n.dat' -> (501, 59, 'busm1', 'Warn_n'); None for other entries."""
    m = CLIP.match(name)
    if not m:
        return None
    return int(m.group(1)), int(m.group(2)), m.group(3), m.group(4)


def sth_takes(sth: bytes, dat_size: int) -> list[dict]:
    """The `.sth` rows (u32 offset in the .dat + 8-byte EA SNR header): per take its offset, size,
    codec byte, channels, rate and sample count."""
    rows = [sth[i:i + 12] for i in range(0, len(sth) - 11, 12)]
    offsets = [struct.unpack('>I', r[:4])[0] for r in rows] + [dat_size]
    takes = []
    for i, r in enumerate(rows):
        w1, w2 = struct.unpack('>II', r[4:12])
        takes.append({'offset': offsets[i], 'size': offsets[i + 1] - offsets[i], 'codec': w1 >> 24,
                      'channels': ((w1 >> 18) & 0x3F) + 1, 'rate': w1 & 0x3FFFF, 'samples': w2 & 0x1FFFFFFF,
                      'snr': r[4:12].hex()})
    return takes


def _nested(archive, path: str):
    """An EB v3 archive stored inside another one."""
    import tempfile
    from tools.owned_game.big import BigArchive
    entry = next(e for e in archive.entries if e.path == path)
    tmp = Path(tempfile.mkdtemp()) / Path(path).name
    tmp.write_bytes(archive.read(entry))
    return BigArchive(tmp)


def hdr_fields(hdr: bytes) -> dict:
    """A clip's `.hdr` (the speech library's per-clip header, recomp sub_82972660 / sub_82973408):
    u16 id (the `.evt` records name clips by it), +2 flags (bit 7: take-condition bits; low 7 bits:
    condition bytes per take), +3 take count, +8 length of the clip's take history (a ring of the
    takes it last played)."""
    return {'id': struct.unpack('>H', hdr[:2])[0], 'takes': hdr[3], 'history': hdr[8], 'flags': hdr[2]}


def _align4(n: int) -> int:
    return (n + 3) & ~3


def parse_evt(data: bytes) -> dict:
    """The speech library's event table (`<prefix>_Events.evt`, big-endian; recomp sub_829711D0,
    sub_82972980, sub_82973BD8, sub_82972D70). Header: +8 bank, +9 sub-bank, +0x10 u16 event count,
    +0x18 u16 event offsets (× 4), +4 u32 offset of the name table (32-byte rows, u32 string offset).
    Event: u16 id, u16 queue timeout, u16 priority, u8 record count, u8 external-condition count,
    u8 flags (high nibble = field count), u8 probability %, u8 flags2, u8, u16 record offsets (× 4,
    from the event), then 3-byte field descriptors (0xFF, field id = request word, size).
    Record: u8 weight code (4^(b >> 5) × (b & 31)), u8 probability %, u8 clips << 2 | mode, u8 locals,
    u8 field count, 3 pad, one byte per clip (its offset × 4 from the record), then u32 field values
    (0 = any; else a bit mask the request word must share), then 8-byte clip entries (u16 clip id,
    u8, u8 lookup mode, i8 parameter count, …)."""
    u16 = lambda o: struct.unpack_from('>H', data, o)[0]  # noqa: E731
    u32 = lambda o: struct.unpack_from('>I', data, o)[0]  # noqa: E731
    names_at, count = u32(4), u16(0x10)
    strings = names_at + 32 * count
    events = []
    for i in range(count):
        o = u16(0x18 + 2 * i) * 4
        name_at = strings + u32(names_at + 32 * i)
        name = data[name_at:data.index(b'\0', name_at)].decode('ascii', 'replace')
        n_records, n_conditions, flags = data[o + 6], data[o + 7], data[o + 8]
        n_fields = flags >> 4
        # Record offsets, the per-record condition bits and the 3-byte condition descriptors, each
        # padded to 4 bytes (sub_82973BD8's arithmetic), then the field descriptors.
        fields_at = (o + 12 + _align4(2 * n_records) + _align4((n_conditions + 7) // 8 * n_records * 2)
                     + _align4(3 * n_conditions))
        fields = [data[fields_at + 3 * k + 1] for k in range(n_fields)]
        records = []
        for k in range(n_records):
            r = o + 4 * u16(o + 12 + 2 * k)
            n_clips, n_values = data[r + 2] >> 2, data[r + 4]
            values_at = r + 8 + _align4(n_clips)
            clips = [u16(r + 4 * data[r + 8 + c]) for c in range(n_clips)]
            records.append({'weight': data[r], 'probability': data[r + 1], 'mode': data[r + 2] & 3, 'locals': data[r + 3],
                            'values': [u32(values_at + 4 * v) for v in range(n_values)], 'clips': clips,
                            'clip_entries': [data[r + 4 * data[r + 8 + c]:r + 4 * data[r + 8 + c] + 8].hex() for c in range(n_clips)]})
        events.append({'id': u16(o), 'name': name, 'queue_timeout': u16(o + 2), 'priority': u16(o + 4),
                       'conditions': n_conditions, 'flags': flags, 'probability': data[o + 9], 'flags2': data[o + 10],
                       'byte11': data[o + 11], 'fields': fields, 'records': records})
    return {'bank': data[8], 'sub_bank': data[9], 'events': events}


def speech_index(archive_path: Path, prefix: str = 'livingworld') -> dict:
    """{'archive': name, 'clips': [{name, event, voice, voice_name, line, dat_offset, id, history, takes: [...]}],
    'rules': parse_evt(<prefix>_Events.evt)}."""
    from tools.owned_game.big import BigArchive
    archive = BigArchive(archive_path)
    sth = _nested(archive, f'{prefix}sth.big')
    rows = {Path(e.path).stem: sth.read(e) for e in sth.entries}
    hdr = _nested(archive, f'{prefix}hdr.big')
    headers = {Path(e.path).stem: hdr_fields(hdr.read(e)) for e in hdr.entries}
    clips = []
    for e in sorted(archive.entries, key=lambda e: e.path):
        parsed = parse_clip_name(Path(e.path).name)
        if not parsed or Path(e.path).stem not in rows:
            continue
        event, voice, voice_name, line = parsed
        takes = sth_takes(rows[Path(e.path).stem], e.stored_size)
        h = headers.get(Path(e.path).stem, {})
        clips.append({'name': Path(e.path).name, 'event': event, 'voice': voice, 'voice_name': voice_name, 'line': line,
                      'dat_offset': e.offset, 'id': h.get('id'), 'history': h.get('history', 0),
                      'takes': [{k: t[k] for k in ('offset', 'size', 'rate', 'samples', 'snr')} for t in takes]})
    out = {'archive': archive_path.name, 'clips': clips}
    evt = next((e for e in archive.entries if e.path == f'{prefix}_Events.evt'), None)
    if evt is not None:
        out['rules'] = parse_evt(archive.read(evt))
    return out


# Presentation speech lines (Speech / SpeechNoWait events; doc 42 "Presentation speech", research b874): nis.big holds
# one bare .sns body per line named <character>_<speech id>_<category>-<tag>_<take>[_360] and nisresident.big one
# 8-byte .snr header per line. The character is the eSk8Characters id (menu-tables.json "speakers").
NIS_LINE = re.compile(r'(\d+)_(\d+)_([A-Za-z0-9]+)-([A-Za-z0-9-]+)_(\d+)(_360)?\.sns')


def nis_index(archive_path: Path) -> dict:
    """{'archive': name, 'lines': [{file, character, id, category, tag, take, x360, snr}]} in archive order."""
    from tools.owned_game.big import BigArchive
    archive = BigArchive(archive_path)
    resident = _nested(archive, 'nisresident.big')
    headers = {Path(e.path).stem: resident.read(e) for e in resident.entries}
    sns = {Path(e.path).stem for e in archive.entries if e.path.endswith('.sns')}
    lines = []
    # Retail (824A5A08, research b1014) walks nisresident.big's file table in order and reads the character from
    # name chars 0..1 and the speech id from chars 3..6; the order is the take order of a line.
    for e in resident.entries:
        stem = Path(e.path).stem
        try:
            character, speech_id = int(stem[0:2]), int(stem[3:7])
        except ValueError:
            continue
        if stem not in sns:
            continue
        m = NIS_LINE.fullmatch(stem + '.sns')
        lines.append({'file': stem, 'character': character, 'id': speech_id, 'category': m[3] if m else '',
                      'tag': m[4] if m else '', 'take': int(m[5]) if m else 0, 'x360': bool(m and m[6]),
                      'snr': headers[stem].hex()})
    return {'archive': archive_path.name, 'lines': lines}


def decode_nis(archive_path: Path, index: dict, output: Path, work: Path, vgmstream: Path, decode, log) -> int:
    """Decode every line to output/<file>.wav (`decode` = audio_export._decode), one worker per character.
    Returns the number of lines written."""
    from tools.owned_game.big import BigArchive
    from .audio_export import _in_order
    archive = BigArchive(archive_path)
    by_name = {Path(e.path).name: e for e in archive.entries}
    groups: dict[int, list] = {}
    for line in index['lines']:
        groups.setdefault(line['character'], []).append(line)
    output.mkdir(parents=True, exist_ok=True)

    def decode_group(character):
        folder = work / str(character)
        folder.mkdir(parents=True, exist_ok=True)
        names = []
        for line in groups[character]:
            (folder / f"{line['file']}.snr").write_bytes(bytes.fromhex(line['snr']))
            (folder / f"{line['file']}.sns").write_bytes(archive.read(by_name[line['file'] + '.sns']))
            names.append(f"{line['file']}.snr")
        decode(vgmstream, folder, names, log)
        for line in groups[character]:
            (folder / f"{line['file']}.snr.wav").replace(output / f"{line['file']}.wav")
        return len(groups[character])
    return sum(_in_order(decode_group, groups))


def speech_requested() -> bool:
    # On by default (USER-ASKS 338); the game passes SKATE_SETUP_SPEECH=0 when the player switched Ped Speech off.
    return os.environ.get('SKATE_SETUP_SPEECH', '') != '0'


def decode_speech(archive_path: Path, index: dict, output: Path, work: Path, vgmstream: Path, decode, log,
                  events=FREE_ROAM_EVENTS) -> int:
    """Decode the takes of `events` to output/<clip stem>/<take>.wav (`decode` = audio_export._decode).
    Returns the number of takes written."""
    from tools.owned_game.big import BigArchive
    from .audio_export import _in_order
    archive = BigArchive(archive_path)
    by_name = {Path(e.path).name: e for e in archive.entries}
    # Clips decode on the audio decode workers (each a vgmstream process; doc 08 "Speech decode in parallel"). Clips
    # sharing a folder stay together in their index order, so the result is the same as one after another.
    groups: dict[str, list] = {}
    for clip in index['clips']:
        if clip['event'] in events:
            groups.setdefault(Path(clip['name']).stem, []).append(clip)

    def decode_group(stem):
        written = 0
        for clip in groups[stem]:
            data = archive.read(by_name[clip['name']])
            folder = work / stem
            folder.mkdir(parents=True, exist_ok=True)
            names = []
            for i, t in enumerate(clip['takes']):
                (folder / f'{i:02d}.snr').write_bytes(bytes.fromhex(t['snr']))
                (folder / f'{i:02d}.sns').write_bytes(data[t['offset']:t['offset'] + t['size']])
                names.append(f'{i:02d}.snr')
            decode(vgmstream, folder, names, log)
            target = output / stem
            target.mkdir(parents=True, exist_ok=True)
            for i in range(len(clip['takes'])):
                (folder / f'{i:02d}.snr.wav').replace(target / f'{i:02d}.wav')
                written += 1
        return written
    return sum(_in_order(decode_group, groups))
