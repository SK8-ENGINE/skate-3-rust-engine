"""World sound sources' setup helpers, checked on synthetic data (never game data)."""
import struct
import unittest

from tools.asset_pipeline import world_audio as world


class ClipNames(unittest.TestCase):
    def test_names_split_into_event_voice_and_line(self):
        self.assertEqual(world.parse_clip_name('501_59_busm1_Warn_n.dat'), (501, 59, 'busm1', 'Warn_n'))
        self.assertEqual(world.parse_clip_name('1901_53_Shout.dat'), (1901, 53, None, 'Shout'))
        self.assertEqual(world.parse_clip_name('806_47_adtf2_Int_c14_tour.dat'), (806, 47, 'adtf2', 'Int_c14_tour'))
        self.assertIsNone(world.parse_clip_name('livingworld_Events.evt'))
        self.assertIsNone(world.parse_clip_name('livingworldhdr.big'))


class StreamTable(unittest.TestCase):
    def test_rows_give_offsets_sizes_and_snr_fields(self):
        # Two takes: mono 36 kHz codec 3, 1000 and 500 samples, at 0 and 0x100 of a 0x180-byte clip.
        rows = b''
        for offset, samples in ((0, 1000), (0x100, 500)):
            rows += struct.pack('>III', offset, (3 << 24) | 36000, (1 << 30) | samples)
        takes = world.sth_takes(rows, 0x180)
        self.assertEqual([(t['offset'], t['size'], t['samples']) for t in takes], [(0, 0x100, 1000), (0x100, 0x80, 500)])
        self.assertEqual((takes[0]['codec'], takes[0]['channels'], takes[0]['rate']), (3, 1, 36000))
        self.assertEqual(takes[1]['snr'], struct.pack('>II', (3 << 24) | 36000, (1 << 30) | 500).hex())


class Tuning(unittest.TestCase):
    def test_engine_records_inherit_from_their_parent(self):
        f32 = lambda v: struct.pack('>f', v).hex()  # noqa: E731
        collections = [
            {'class': world.ENGINE_CLASS, 'key': 'default', 'parent': '', 'fields': {
                'Hash_10C7F64B3253B21F': {'type': 'EA::Reflection::Float', 'data': f32(850)},
                'Hash_DD02885FAFA71D6D': {'type': 'EA::Reflection::Float', 'data': f32(4000)},
                'Hash_C436B6BC22BC023C': {'type': 'EA::Reflection::Int16', 'data': '0002'},
                'Hash_07CE76F8BE0066C1': {'type': 'EA::Reflection::Int16', 'data': '00040000'},
                'Hash_763DB0A168A49E93': {'type': 'EA::Reflection::Int32', 'data': '00004E20'}}},
            {'class': world.ENGINE_CLASS, 'key': 'Hash_0000000000000001', 'parent': 'default', 'fields': {
                'Hash_10C7F64B3253B21F': {'type': 'EA::Reflection::Float', 'data': f32(1500)},
                'Hash_C436B6BC22BC023C': {'type': 'EA::Reflection::Int16', 'data': '0004'}}},
        ]
        t = world.world_tuning(collections)
        child = t['traffic_engine']['Hash_0000000000000001']
        self.assertEqual((child['idle_rpm'], child['max_rpm'], child['patch'], child['gears'], child['rear_bias']), (1500.0, 4000.0, 4, 4, 20000))
        self.assertEqual(t['traffic_engine']['default']['patch'], 2)
        self.assertEqual(t['ped_footsteps'], {})


def _evt(event_id, fields, records, name='501_warn'):
    """A one-event `.evt` in the retail layout (synthetic). records = [(values, clip ids)]."""
    def align(n):
        return (n + 3) & ~3
    head = 12 + align(2 * len(records)) + align(3 * len(fields))
    recs, offsets = b'', []
    for values, clips in records:
        offsets.append((head + len(recs)) // 4)
        values_at = 8 + align(len(clips))
        clips_at = values_at + 4 * len(values)
        r = bytes([0x39, 100, len(clips) << 2, 0, len(values), 0, 0, 0])
        r += bytes((clips_at + 8 * c) // 4 for c in range(len(clips)))
        r = r.ljust(values_at, b'\0') + b''.join(struct.pack('>I', v) for v in values)
        r += b''.join(struct.pack('>H', c) + bytes(6) for c in clips)
        recs += r
    body = struct.pack('>HHHBBBBBB', event_id, 60, 500, len(records), 0, len(fields) << 4, 100, 0, 0)
    body += b''.join(struct.pack('>H', o) for o in offsets)
    body = body.ljust(12 + align(2 * len(records)), b'\0') + b''.join(bytes([0xFF, f, 4]) for f in fields)
    body = body.ljust(head, b'\0') + recs
    event_at = 0x1C
    names_at = event_at + align(len(body))
    out = bytearray(event_at)
    out[4:8] = struct.pack('>I', names_at)
    out[8] = 1
    out[0x10:0x12] = struct.pack('>H', 1)
    out[0x18:0x1A] = struct.pack('>H', event_at // 4)
    out += body
    out = out.ljust(names_at + 32, b'\0') + name.encode() + b'\0'
    return bytes(out)


class SpeechRules(unittest.TestCase):
    def test_evt_records_give_values_and_clip_sequences(self):
        data = _evt(0x2012, [1, 2, 3], [([4, 1, 2], [0x2FE2]), ([0x4000, 2, 0], [0x3AF6, 0x2D9F, 0x3AF6])])
        rules = world.parse_evt(data)
        self.assertEqual(rules['bank'], 1)
        (ev,) = rules['events']
        self.assertEqual((ev['id'], ev['name'], ev['fields'], ev['probability']), (0x2012, '501_warn', [1, 2, 3], 100))
        self.assertEqual([(r['values'], r['clips'], r['weight']) for r in ev['records']],
                         [([4, 1, 2], [0x2FE2], 0x39), ([0x4000, 2, 0], [0x3AF6, 0x2D9F, 0x3AF6], 0x39)])

    def test_hdr_fields(self):
        hdr = struct.pack('>HBB', 0x3454, 0, 8) + b'\xff' * 4 + bytes([8, 0]) + b'\x10\x37'
        self.assertEqual(world.hdr_fields(hdr), {'id': 0x3454, 'takes': 8, 'history': 8, 'flags': 0})

    def test_speech_tuning_reads_the_struct_and_inherits(self):
        raw = bytearray(64)
        struct.pack_into('>f', raw, 8, 10.0)
        struct.pack_into('>i', raw, 16, 100)
        struct.pack_into('>ff', raw, 20, 100.0, 15.0)
        raw[60] = 1
        default = bytearray(64)
        struct.pack_into('>i', default, 16, 50)
        struct.pack_into('>f', default, 20, 100.0)
        collections = [
            {'class': world.SPEECH_CLASS, 'key': 'default', 'parent': '', 'fields': {
                world.SPEECH_TUNING: {'type': 'Sk8::Audio::tSpeechTuning', 'data': default.hex()}}},
            {'class': world.SPEECH_CLASS, 'key': 'Hash_01', 'parent': 'default', 'fields': {
                'Hash_A': {'type': 'SPCHType_1_EventID', 'data': '00002012'},
                world.SPEECH_TUNING: {'type': 'Sk8::Audio::tSpeechTuning', 'data': raw.hex()},
                'Hash_B': {'type': 'Sk8::Audio::tLWNotFollow', 'data': '0001000100080000',
                           'array': {'items': [struct.pack('>if', 0x2002, 30.0).hex()]}}}},
            {'class': world.SPEECH_CLASS, 'key': 'Hash_02', 'parent': 'default', 'fields': {
                'Hash_A': {'type': 'SPCHType_1_EventID', 'data': '00002054'}}},
        ]
        t = world.speech_tuning(collections)['1']
        warn = t[str(0x2012)]
        self.assertEqual((warn['gap'], warn['priority'], warn['probability'], warn['repeat'], warn['zombie']), (10.0, 100, 100.0, 15.0, True))
        self.assertEqual(warn['not_follow'], [[0x2002, 30.0]])
        self.assertEqual((t[str(0x2054)]['priority'], t[str(0x2054)]['repeat']), (50, 0.0))


if __name__ == '__main__':
    unittest.main()
