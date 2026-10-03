"""Survey of Skate 3's granular rolling-bed data (grains.big + the vault tuning).

Our own reader (no code copied from the game). Writes a small JSON summary to
.local/audio-file-inspect/grain_survey.json (--out DIR):

- per .grain member: header length, stored duration, seek-table header, every seek-table row
  (byte step, side step, samples, key flag), side-data size, EAAC header (codec, channels,
  rate, samples) and whether row 0 spans the whole stream;
- per grain-class vault collection (class 0x7AB23C11B6ADA2DE): filename, max km/h, Bezier
  control points, GrainParams, intensity cap, boost/shift values, push envelope values;
- the owner/rocket tuning (class 0x6E878344774A7999 'default').

Optional --rms: decode each member with vgmstream-cli (VGMSTREAM env, PATH or the one setup downloads)
and add an RMS-vs-normalised-position profile (20 bins) so the "slow -> fast sweep" can be quantified.
The vault part reads the install's converted database (assets/private/stock/skater-collections.json, --vault).

Usage (repo root):
  py -3.13 tools/audio-file-inspect/grain_survey.py [--rms] [--disc DIR] [--vault JSON] [--out DIR]
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.owned_game.big import BigArchive  # noqa: E402

DISC_AUDIO = Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')) / 'data' / 'audio'
VAULT = REPO / 'assets' / 'private' / 'stock' / 'skater-collections.json'
OUT = REPO / '.local' / 'audio-file-inspect'

GRAIN_CLASS = 'Hash_7AB23C11B6ADA2DE'
OWNER_CLASS = 'Hash_6E878344774A7999'

# Vault field hashes of the grain class (descriptive names, as used by the engine's grain player).
GRAIN_FIELDS = {
    'Hash_2C073BF8BC45063B': 'grain_file',
    'Hash_4890392C91829954': 'max_kmh',
    'Hash_CEC749561306022A': 'b_slope_gain',
    'Hash_D380D303C64CF6F8': 'b_slope_ramp_kmh',
    'Hash_1F459FC797B2C6BA': 'a_shift_per_slope_hz',
    'Hash_5C9AA28695C17004': 'turn_intensity_cap',
    'Hash_145D8340A9440DA3': 'b_base_shift_hz',
    'Hash_7FFF3A8AD44809EF': 'b_shift_per_slope_hz',
    'Hash_281FF01081475899': 'turn_rise_step',
    'Hash_63764B8C7EB9EC9B': 'turn_fall_step',
    'Hash_6BDC44AE7C3C79D0': 'special_gain_a',
    'Hash_F62BC5EBD8E5DDE8': 'special_shift_a_hz',
    'Hash_2D751DEB89BB5E33': 'push_ramp_kmh',
    'Hash_E239B03F0E890686': 'push_scale_low',
    'Hash_B87ECDDAAB0F8404': 'push_scale_high',
    'Hash_C658A7923FC7B99E': 'push_shift_low_hz',
    'Hash_A15AD56E225ADBA6': 'push_shift_high_hz',
    'Hash_B3D7468820AFC661': 'push_scale_attack_ms',
    'Hash_DAC9DA910EF0316C': 'push_scale_hold_ms',
    'Hash_0C3D5DBC262ED276': 'push_scale_return_ms',
    'Hash_09A5CC79BA2178E7': 'push_shift_attack_ms',
    'Hash_3206FD96427EA4D2': 'push_shift_hold_ms',
    'Hash_DB597F672CA47138': 'push_shift_return_ms',
    'Hash_57A78D3BE8D47BB3': 'slope_down_divisor',
    'Hash_8DD4C3FC8DAF4059': 'slope_up_divisor',
    'Hash_12275AA8AC4A63FB': 'rattle_kmh',
}
OWNER_FIELDS = {
    'Hash_7508154FF73DDCED': 'rocket_start_kmh',
    'Hash_1185E9A69919B051': 'rocket_top_kmh',
    'Hash_9BC13FA19CC4DF00': 'rocket_gain_word',
    'Hash_E64C04ED542DABC8': 'graph3_clip',
    'Hash_55BEB30353F244A9': 'graph3_shelf_hz',
    'Hash_45516395725ED16B': 'graph3_shelf_gain',
    'Hash_0D665393E2EDC605': 'g3_send_low_kmh',
    'Hash_28E708782445747F': 'g3_send_high_kmh',
    'Hash_D900C07BE7C5450F': 'g3_send_level',
    'Hash_88AA96B08FD16914': 'g1_level_start_kmh',
    'Hash_3FFB5107C82BA3E0': 'g1_level_end_kmh',
    'Hash_D3E8894CA25A4F71': 'g1_level_floor',
    'Hash_281A501B22B6CCDF': 'wobble_low_kmh',
    'Hash_54CDE019E31FC04E': 'wobble_high_kmh',
    'Hash_36AE41817640FE04': 'wobble0_ms_low',
    'Hash_71EE27313BD30F21': 'wobble0_ms_high',
    'Hash_437D128B53669C34': 'wobble0_gain_low',
    'Hash_02885338DD5D7DCA': 'wobble0_gain_high',
    'Hash_2055BBF39C152FA9': 'wobble1_ms_low',
    'Hash_F5240AFADA3B3FFC': 'wobble1_ms_high',
    'Hash_F916E153393C5F24': 'wobble1_gain_low',
    'Hash_0A36F90732016D85': 'wobble1_gain_high',
}


def be32(b: bytes, o: int) -> int:
    return struct.unpack_from('>I', b, o)[0]


def f32(word: str) -> float:
    return struct.unpack('>f', bytes.fromhex(word))[0]


class Cursor:
    def __init__(self, data: bytes, pos: int):
        self.data, self.pos = data, pos

    def varint(self) -> int:
        """Signed variable-length integer (1..5 bytes, form chosen by the first byte)."""
        d, p = self.data, self.pos
        b0 = d[p]
        if b0 < 0xC0:
            mag, sign, n = b0 >> 1, b0 & 1, 1
        elif b0 < 0xF0:
            w = ((b0 << 8) | d[p + 1]) >> 1
            mag, sign, n = (w & ~0x6000) + 96, d[p + 1] & 1, 2
        elif b0 < 0xFC:
            w = (((b0 << 8) | d[p + 1]) & ~0xF000) << 8 | d[p + 2]
            mag, sign, n = (w >> 1) + 6240, d[p + 2] & 1, 3
        elif b0 < 0xFF:
            w = ((b0 & 3) << 24) | (d[p + 1] << 16) | (d[p + 2] << 8) | (d[p + 3] & 0xFE)
            mag, sign, n = (w >> 1) + 0x60000 + 6240, d[p + 3] & 1, 4
        else:
            self.pos += 5
            v = be32(d, p + 1)
            return v - (1 << 32) if v & 0x80000000 else v
        self.pos += n
        return -1 - mag if sign else mag


class Column:
    """Run-length column: a header h >= 0 repeats one delta for h+1 rows; h < 0 gives 1-h
    rows that each read their own delta. The column value is the running sum of deltas."""

    def __init__(self, cur: Cursor):
        self.cur, self.value, self.left, self.repeat = cur, 0, 0, False

    def next(self) -> int:
        if self.left <= 0:
            h = self.cur.varint()
            if h >= 0:
                self.left, self.repeat = h + 1, True
                self.value += self.cur.varint()
            else:
                self.left, self.repeat = 1 - h, False
        if not self.repeat:
            self.value += self.cur.varint()
        self.left -= 1
        return self.value


def seek_rows(table: bytes, limit: int = 4096) -> list[list[int]]:
    cur = Cursor(table, 8)
    cols = [Column(cur) for _ in range(4)]
    rows = []
    while len(rows) < limit and cur.pos < len(table):
        try:
            row = [c.next() for c in cols]
        except IndexError:
            break
        rows.append(row)
        if row[2] < 0:
            break
    return rows


def eaac(b: bytes, at: int) -> dict:
    w0, w1 = be32(b, at), be32(b, at + 4)
    return {
        'version': w0 >> 28, 'codec': (w0 >> 24) & 0xF, 'channels': ((w0 >> 18) & 0x3F) + 1,
        'rate': w0 & 0x3FFFF, 'type': w1 >> 30, 'loop': (w1 >> 29) & 1, 'samples': w1 & 0x1FFFFFFF,
    }


def survey_member(name: str, b: bytes) -> dict:
    h = be32(b, 0)
    duration = struct.unpack_from('>f', b, 4)[0]
    table = b[8:h]
    side_off = be32(table, 4)
    s = eaac(b, h)
    rows = seek_rows(table[:side_off] if side_off else table)
    first = rows[0] if rows else None
    return {
        'name': name, 'bytes': len(b), 'header_len': h,
        'duration_word': f'{be32(b, 4):08X}', 'duration_s': duration,
        'seek_kind': table[0], 'seek_layout': table[1] >> 4, 'seek_low': table[1] & 0xF,
        'preroll': struct.unpack_from('>H', table, 2)[0], 'side_offset': side_off,
        'side_bytes': (len(table) - side_off) if side_off else 0,
        'rows_read': len(rows), 'row0': first,
        'row0_spans_stream': bool(first and first[2] == s['samples'] and first[3] == 1),
        'stream_bytes_after_header': len(b) - h,
        'eaac': s, 'samples_over_rate': s['samples'] / s['rate'],
        'duration_ulps_off': abs(struct.unpack('>i', struct.pack('>f', duration))[0]
                                 - struct.unpack('>i', struct.pack('>f', s['samples'] / s['rate']))[0]),
    }


def decode_value(field: dict):
    t, data = field.get('type', ''), field.get('data', '')
    if t.endswith('Float'):
        return f32(data)
    if t.endswith('Int32'):
        v = int(data, 16)
        return v - (1 << 32) if v & 0x80000000 else v
    if t.endswith('Text'):
        return data
    return data


def vault_rows():
    rows = json.loads(VAULT.read_text())['collections']
    return [r for r in rows if r['class'] in (GRAIN_CLASS, OWNER_CLASS)]


def resolve(rows, cls, key, field):
    by_key = {r['key']: r for r in rows if r['class'] == cls}
    seen = 0
    while key and key in by_key and seen < 32:
        r = by_key[key]
        if field in r['fields']:
            return r['fields'][field], key
        key, seen = r.get('parent', ''), seen + 1
    return None, None


def grain_params(field: dict) -> list[list[float]]:
    items = field['array']['items']
    return [[f32(it[k * 8:k * 8 + 8]) for k in range(5)] for it in items]


def survey_vault() -> dict:
    rows = vault_rows()
    out = {'grain_class': {}, 'owner_default': {}}
    for r in (r for r in rows if r['class'] == GRAIN_CLASS):
        key = r['key']
        entry = {'parent': r.get('parent', '')}
        for h, name in GRAIN_FIELDS.items():
            field, src = resolve(rows, GRAIN_CLASS, key, h)
            if field is not None:
                entry[name] = decode_value(field)
                if src != key:
                    entry.setdefault('inherited', []).append(name)
        m, _ = resolve(rows, GRAIN_CLASS, key, 'Hash_A985FBAA9326718D')
        if m:
            vals = [f32(m['data'][i * 8:i * 8 + 8]) for i in range(16)]
            entry['matrix_rows'] = [vals[i * 4:i * 4 + 4] for i in range(4)]
            # column 1 of rows 0..3 = end, 2nd, 1st, start control point of the position curve
            entry['bezier_p0_to_p3'] = [vals[13], vals[9], vals[5], vals[1]]
            entry['matrix_col0_rows'] = [vals[0], vals[4], vals[8], vals[12]]
        gp, _ = resolve(rows, GRAIN_CLASS, key, 'Hash_D18D1174735E5CDE')
        if gp:
            entry['grain_params_A_B'] = grain_params(gp)
        out['grain_class'][key] = entry
    for h, name in OWNER_FIELDS.items():
        field, _ = resolve(rows, OWNER_CLASS, 'default', h)
        if field is not None:
            out['owner_default'][name] = decode_value(field)
    # Sk8::AudioSurfaceMap (holder class, key C489459A0C07D154, field 4CA607558B1CF440): one
    # 18-word element per collision material 0..94; word 1 (+4) = rolling surface 1..14.
    allrows = json.loads(VAULT.read_text())['collections']
    smap = next((r for r in allrows if r['class'] == 'Hash_C1831BDB6CB1B1EA'
                 and r['key'] == 'Hash_C489459A0C07D154'), None)
    if smap and 'Hash_4CA607558B1CF440' in smap['fields']:
        items = smap['fields']['Hash_4CA607558B1CF440']['array']['items']
        table = {}
        for i, it in enumerate(items):
            hx = ''.join(it.split())
            table[i] = int(hx[8:16], 16)
        out['material_to_rolling_surface'] = table
    gp, _ = resolve(rows, OWNER_CLASS, 'default', 'Hash_D18D1174735E5CDE')
    if gp:
        out['owner_default']['rocket_grain_params'] = (
            grain_params(gp) if 'array' in gp else [f32(gp['data'][k * 8:k * 8 + 8]) for k in range(5)])
    return out


def rms_profile(name: str, b: bytes, bins: int = 20) -> dict | None:
    exe = os.environ.get('VGMSTREAM') or shutil.which('vgmstream-cli')
    bundled = REPO / 'data' / 'tools' / 'vgmstream-cli' / 'vgmstream-cli.exe'
    if not exe and bundled.is_file():
        exe = str(bundled)
    if not exe:
        return None
    import wave
    import numpy
    h = be32(b, 0)
    with tempfile.TemporaryDirectory() as tmp:
        src, dst = Path(tmp) / f'{Path(name).stem}.snr', Path(tmp) / 'out.wav'
        src.write_bytes(b[h:])
        if subprocess.run([exe, '-o', str(dst), str(src)], capture_output=True).returncode:
            return None
        with wave.open(str(dst)) as w:
            x = numpy.frombuffer(w.readframes(w.getnframes()), '<i2').astype(numpy.float64) / 32768
            rate = w.getframerate()
    size = len(x) // bins
    rms, centroid = [], []
    for i in range(bins):
        seg = x[i * size:(i + 1) * size]
        rms.append(round(float(20 * numpy.log10(numpy.sqrt(numpy.mean(seg ** 2)) + 1e-12)), 2))
        spec = numpy.abs(numpy.fft.rfft(seg * numpy.hanning(len(seg))))
        freqs = numpy.fft.rfftfreq(len(seg), 1 / rate)
        centroid.append(round(float((spec * freqs).sum() / (spec.sum() + 1e-12)), 1))
    return {'rms_db': rms, 'centroid_hz': centroid, 'decoded_frames': len(x)}


def main() -> None:
    global DISC_AUDIO, VAULT, OUT
    parser = argparse.ArgumentParser(description="Survey the granular rolling-bed data (grains.big + vault tuning).")
    parser.add_argument('--rms', action='store_true', help='decode each member and add an RMS / centroid profile')
    parser.add_argument('--disc', type=Path, help='extracted disc root, the folder holding data/')
    parser.add_argument('--vault', type=Path, default=VAULT, help='converted skater-collections.json (default: %(default)s)')
    parser.add_argument('--out', type=Path, default=OUT, help='output folder (default: %(default)s)')
    args = parser.parse_args()
    if args.disc:
        DISC_AUDIO = args.disc / 'data' / 'audio'
    VAULT, OUT = args.vault, args.out
    OUT.mkdir(parents=True, exist_ok=True)
    big = BigArchive(DISC_AUDIO / 'grains.big')
    members = []
    for e in big.entries:
        if not e.path.lower().endswith('.grain'):
            continue
        b = big.read(e)
        m = survey_member(e.path, b)
        if args.rms:
            m['profile'] = rms_profile(e.path, b)
        members.append(m)
    result = {'members': members, 'vault': survey_vault()}
    (OUT / 'grain_survey.json').write_text(json.dumps(result, indent=1))
    for m in members:
        print(f"{m['name']:34} H={m['header_len']:3} dur={m['duration_s']:7.3f}s "
              f"rate={m['eaac']['rate']} n={m['eaac']['samples']} rows={m['rows_read']} "
              f"row0={m['row0']} spans={m['row0_spans_stream']} side={m['side_bytes']} "
              f"ulps={m['duration_ulps_off']}")
        if m.get('profile'):
            print('   rms_db', m['profile']['rms_db'])
            print('   centroid_hz', m['profile']['centroid_hz'])
    for k, v in result['vault']['grain_class'].items():
        print(k, v.get('grain_file'), v.get('max_kmh'), [round(p, 4) for p in v.get('bezier_p0_to_p3', [])],
              v.get('turn_intensity_cap'), v.get('b_slope_gain'), v.get('b_slope_ramp_kmh'),
              v.get('a_shift_per_slope_hz'), v.get('b_base_shift_hz'))
    print('owner', result['vault']['owner_default'])


if __name__ == '__main__':
    main()
