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
- `decode_speech(...)`: opt-in (`SKATE_SETUP_SPEECH=1`): each take of the chosen events decoded to a
  mono PCM16 WAV at its own 36 kHz (`speech/livingworld/<clip>/<take>.wav`). All free-roam events are
  ~9 h of audio (~2.4 GB), so the default setup leaves it out.

Reading of the disc's own data at setup time; nothing from the game is committed. Formats and the
mechanism: .claude/notes/world-speech.md, world-traffic-audio.md, world-ped-audio.md.
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
    'car_alarms.abk', 'fstep_livingworld.abk', 'Tazer.abk',
)

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

# The speech events a free-roam ped can say (world-speech.md): reactions, chases, conversations, phone
# calls, bums, the player-action comments.
FREE_ROAM_EVENTS = (101, 102, 104, 105, 108, 109, 110, 201, 202, 203, 204, 205, 206, 207, 314, 315, 316, 320,
                    330, 331, 335, 336, 338, 339, 400, 497, 501, 603, 604, 605, 606, 607, 609, 611, 805, 806, 807,
                    1901, 4402, 4405)


def _word(data: str, kind: str):
    raw = bytes.fromhex(''.join(data.split()))
    if kind == 'f32':
        return round(struct.unpack('>f', raw[:4])[0], 6)
    if kind == 'i16':
        return struct.unpack('>h', raw[:2])[0]
    return struct.unpack('>i', raw[:4])[0]


def world_tuning(collections: list[dict], record_names: list[str] | None = None) -> dict:
    """{'traffic_engine': {name: {...}}, 'ped_footsteps': {...}} from the converted collections."""
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
    out['speech_tuning'] = speech_tuning(collections)
    return out


# The speech manager's event tuning (vault class read by recomp sub_824ABA18 / sub_824A75F0 /
# sub_824A8C78; .claude/notes/world-speech.md "Speech manager gate").
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


def speech_requested() -> bool:
    return os.environ.get('SKATE_SETUP_SPEECH', '') == '1'


def decode_speech(archive_path: Path, index: dict, output: Path, work: Path, vgmstream: Path, decode, log,
                  events=FREE_ROAM_EVENTS) -> int:
    """Decode the takes of `events` to output/<clip stem>/<take>.wav (`decode` = audio_export._decode).
    Returns the number of takes written."""
    from tools.owned_game.big import BigArchive
    archive = BigArchive(archive_path)
    by_name = {Path(e.path).name: e for e in archive.entries}
    written = 0
    for clip in index['clips']:
        if clip['event'] not in events:
            continue
        data = archive.read(by_name[clip['name']])
        folder = work / Path(clip['name']).stem
        folder.mkdir(parents=True, exist_ok=True)
        names = []
        for i, t in enumerate(clip['takes']):
            (folder / f'{i:02d}.snr').write_bytes(bytes.fromhex(t['snr']))
            (folder / f'{i:02d}.sns').write_bytes(data[t['offset']:t['offset'] + t['size']])
            names.append(f'{i:02d}.snr')
        decode(vgmstream, folder, names, log)
        target = output / Path(clip['name']).stem
        target.mkdir(parents=True, exist_ok=True)
        for i in range(len(clip['takes'])):
            (folder / f'{i:02d}.snr.wav').replace(target / f'{i:02d}.wav')
            written += 1
    return written
