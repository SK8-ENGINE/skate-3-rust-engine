"""EA SNR container helpers, checked on synthetic streams (never game audio)."""
import struct
import unittest

import numpy

from tools.asset_pipeline import audio_formats as audio


def snr(samples=(300, 200), rate=48000, channels=1, codec=3, loop_start=None, last_flag=True):
    """A fake RAM SNR stream: header plus one block per entry in `samples`."""
    header = struct.pack('>I', (codec << 24) | ((channels - 1) << 18) | rate)
    flags = (1 << 29 if loop_start is not None else 0) | sum(samples)
    data = header + struct.pack('>I', flags)
    if loop_start is not None:
        data += struct.pack('>I', loop_start)
    for index, count in enumerate(samples):
        payload = bytes([0x5A]) * (16 + index)
        flag = 0x80 if last_flag and index == len(samples) - 1 else 0x00
        data += struct.pack('>I', (flag << 24) | (8 + len(payload))) + struct.pack('>I', count) + payload
    return data


class SnrStreams(unittest.TestCase):
    def test_reads_header_and_block_chain(self):
        data = b'junk' + snr(rate=44100, channels=2)
        stream = audio.snr_at(data, 4)
        self.assertEqual((stream.channels, stream.sample_rate, stream.samples), (2, 44100, 500))
        self.assertEqual(stream.end, len(data))
        self.assertIsNone(stream.loop_start)
        self.assertEqual(audio.standalone(data, stream), data[4:])

    def test_loop_start_and_unflagged_final_block(self):
        stream = audio.snr_at(snr(loop_start=7, last_flag=False), 0)
        self.assertEqual((stream.loop_start, stream.samples), (7, 500))

    def test_rejects_inconsistent_chains(self):
        good = snr()
        self.assertIsNone(audio.snr_at(good[:-1], 0))                     # truncated block
        self.assertIsNone(audio.snr_at(snr(codec=5), 0))                  # not EA-XMA
        bad_count = bytearray(good)
        struct.pack_into('>I', bad_count, 4, 499)                         # header disagrees with blocks
        self.assertIsNone(audio.snr_at(bytes(bad_count), 0))

    def test_scan_finds_unaligned_back_to_back_streams(self):
        data = b'\x01\x02\x03' + snr() + b'\xff' + snr(samples=(64,)) + b'tail'
        streams = audio.scan_snr(data)
        self.assertEqual([s.offset for s in streams], [3, 3 + len(snr()) + 1])
        self.assertEqual([s.samples for s in streams], [500, 64])

    def test_splc_count_must_match(self):
        header = bytearray(0x40)
        header[:4] = b'SPLC'
        struct.pack_into('>I', header, 0x18, 2)
        bank = bytes(header) + snr() + snr()
        self.assertEqual(len(audio.splc_streams(bank)), 2)
        struct.pack_into('>I', header, 0x18, 3)
        with self.assertRaises(ValueError):
            audio.splc_streams(bytes(header) + snr() + snr())

    def test_grain_offset_and_duration(self):
        stream = snr(samples=(48000,))
        data = struct.pack('>If', 0x20, 1.0) + bytes(0x18) + stream
        parsed = audio.grain(data)
        self.assertEqual((parsed.stream.offset, parsed.stream.samples), (0x20, 48000))
        wrong = struct.pack('>If', 0x20, 2.0) + bytes(0x18) + stream
        with self.assertRaises(ValueError):
            audio.grain(wrong)


class SplcPatches(unittest.TestCase):
    @staticmethod
    def bank():
        # 2 records, 1 container, 3 samples: record 0 = 1 group of 2 members,
        # record 1 = 2 groups of 1 member; container 0 picks record 0 or 1.
        records = []
        for rid, groups in ((0, 1), (1, 2)):
            r = bytearray(36)
            struct.pack_into('>H', r, 4, rid)
            r[7] = groups
            records.append(bytes(r))
        container = bytearray(72)
        struct.pack_into('>HH', container, 4, 0, 1)
        container[68] = 2
        def group(mode, members):
            g = bytearray(12)
            g[8], g[9] = len(members), mode
            out = bytes(g)
            for sample, gain, pitch, gain_range, probability in members:
                m = bytearray(72)
                struct.pack_into('>H', m, 0, sample)
                for offset, value in ((8, gain), (44, pitch), (48, gain_range), (64, probability)):
                    struct.pack_into('>f', m, offset, value)
                out += bytes(m)
            return out
        tree = b''.join(records) + bytes(container) + group(2, [(0, 1.0, 1.2, 0.125, 1.0), (1, 1.0, 1.0, 0.0, 1.0)])             + group(1, [(2, 0.5, 1.0, 0.0, 0.75)]) + group(0, [(1, 1.0, 1.0, 0.0, 1.0)])
        header = bytearray(60)
        header[:4] = b'SPLC'
        struct.pack_into('>IIIII', header, 8, len(tree), 2, 1, 0, 3)
        return bytes(header) + tree

    def test_reads_records_layers_and_containers(self):
        patches = audio.splc_patches(self.bank())
        self.assertEqual(patches['containers'], [[0, 1]])
        self.assertEqual(len(patches['records']), 2)
        first = patches['records'][0]
        self.assertEqual([g['mode'] for g in first], [2])
        self.assertEqual(first[0]['members'], [[0, 1.0, 0.125, 1.2, 1.0], [1, 1.0, 0.0, 1.0, 1.0]])
        self.assertEqual([g['members'][0][0] for g in patches['records'][1]], [2, 1])
        self.assertEqual(patches['records'][1][0]['members'][0][4], 0.75)

    def test_rejects_a_tree_that_does_not_end_at_the_sample_table(self):
        data = bytearray(self.bank())
        struct.pack_into('>I', data, 8, struct.unpack_from('>I', data, 8)[0] + 4)
        with self.assertRaises(ValueError):
            audio.splc_patches(bytes(data))


class Downmix(unittest.TestCase):
    def test_weights_sum_to_one(self):
        for channels in (3, 4, 5, 6):
            for row in audio.downmix_weights(channels):
                self.assertAlmostEqual(sum(row), 1.0, places=6)
        self.assertIsNone(audio.downmix_weights(2))

    def test_five_channel_downmix_never_exceeds_input_peak(self):
        frames = numpy.array([[32767, 32767, 32767, 32767, 32767],
                              [-32768, -32768, -32768, -32768, -32768],
                              [1000, 0, -1000, 500, -500]], dtype='<i2')
        mixed = numpy.frombuffer(audio.downmix_pcm16(frames.tobytes(), 5), dtype='<i2').reshape(-1, 2)
        self.assertEqual(mixed[0].tolist(), [32767, 32767])
        self.assertEqual(mixed[1].tolist(), [-32768, -32768])
        # Left takes L, C and Ls only; right takes C, R and Rs.
        self.assertEqual(mixed[2].tolist(), [round(1500 / 2.7071), round(-1500 / 2.7071)])

    def test_stereo_passes_through(self):
        frames = numpy.array([[1, -2], [3, -4]], dtype='<i2').tobytes()
        self.assertEqual(audio.downmix_pcm16(frames, 2), frames)



class LoopBands(unittest.TestCase):
    def test_bands_wrap_seamlessly_without_gain(self):
        ramp = numpy.arange(6000, dtype=numpy.float32)
        signal = (numpy.sin(ramp * 0.05) * (2000 + ramp * 3)).astype('<i2')
        bands = audio.loop_bands(signal.tobytes(), 3, 100)
        self.assertEqual(len(bands), 3)
        part = signal[:2000].astype(numpy.int32)
        loop = numpy.frombuffer(bands[0], dtype='<i2').astype(numpy.int32)
        self.assertEqual(len(loop), 1900)
        # Wrapping from the loop's end to its start continues the original recording.
        self.assertEqual(loop[-1], part[1899])
        self.assertEqual(loop[0], part[1900])
        self.assertLessEqual(numpy.abs(loop).max(), numpy.abs(part).max())
        # Later bands come from later (louder) parts of the sweep.
        loudness = [numpy.abs(numpy.frombuffer(b, dtype='<i2').astype(numpy.float32)).mean() for b in bands]
        self.assertEqual(loudness, sorted(loudness))

    def test_too_short_recordings_are_rejected(self):
        with self.assertRaises(ValueError):
            audio.loop_bands(bytes(400), 3, 100)


if __name__ == '__main__':
    unittest.main()


class Emitters(unittest.TestCase):
    def test_name_id_matches_the_ids_on_the_disc(self):
        # Sound ids read from the disc's sfx_university.ems next to the bank they name.
        self.assertEqual(audio.name_id('water_fountain'), 0xFAE3503B95E0A3C8)

    def test_name_id_handles_keys_longer_than_one_block(self):
        long = 'a' * 30
        self.assertNotEqual(audio.name_id(long), audio.name_id(long[:24]))
        self.assertEqual(audio.name_id(long), audio.name_id('a' * 30))

    def test_reads_records(self):
        record = audio.EMS_RECORD.pack(7, 0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.5, -0.866, 0.0, 0.5,
                                       audio.name_id('water_fountain'), 1.0, 0.0, 0.5, 1.0)
        emitters = audio.ems_emitters(struct.pack('>I', 2) + record + record)
        self.assertEqual(len(emitters), 2)
        first = emitters[0]
        self.assertEqual((first['index'], first['position'], first['extent']), (7, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]))
        self.assertEqual(first['sound_id'], 0xFAE3503B95E0A3C8)
        self.assertEqual(first['gains'], [1.0, 0.0, 0.5, 1.0])
        self.assertAlmostEqual(first['scalars'][1], -0.866, places=5)

    def test_rejects_a_truncated_file(self):
        with self.assertRaises(ValueError):
            audio.ems_emitters(struct.pack('>I', 2) + bytes(72))


class EmitterAttributes(unittest.TestCase):
    def test_inherited_fields_resolve_through_parents(self):
        from tools.asset_pipeline.audio_export import EMITTER_CLASS, emitter_attributes

        def record(key, parent, **fields):
            return {'class': EMITTER_CLASS, 'key': key, 'parent': parent, 'fields': fields}
        f32 = lambda v: {'type': 'EA::Reflection::Float', 'data': struct.pack('>f', v).hex().upper()}
        i32 = lambda v, t='EA::Reflection::Int32': {'type': t, 'data': struct.pack('>i', v).hex().upper()}
        collections = [
            record('default', '', volume=f32(1.0), Hash_9908F2D75D7381BD=f32(10.0), Hash_6D18B8674D7E5337=i32(0, 'Sk8::Audio::eVolumeType')),
            record('Hash_0000000000000AAA', 'default', Hash_6D18B8674D7E5337=i32(1, 'Sk8::Audio::eVolumeType'),
                   Hash_F209C093F40A4CCC=i32(1, 'Sk8::Audio::eVolumeFalloffType')),
            record('Hash_FAE3503B95E0A3C8', 'Hash_0000000000000AAA', volume=f32(0.5),
                   Hash_BE88128A30BE926E={'type': 'EA::Reflection::Text', 'data': 'water_fountain.abk'},
                   Hash_C493ED34D1D32521=i32(81)),
            {'class': 'Hash_OTHER', 'key': 'Hash_0000000000000BBB', 'parent': '', 'fields': {}},
        ]
        sound = emitter_attributes(collections)[0xFAE3503B95E0A3C8]
        self.assertEqual(sound, {'volume': 0.5, 'seconds': 10.0, 'kind': 1, 'falloff': 1,
                                 'bank_file': 'water_fountain.abk', 'patch': 81})
        self.assertNotIn(0xBBB, emitter_attributes(collections))

    def test_a_reverb_zone_names_its_preset_record(self):
        from tools.asset_pipeline.audio_export import EMITTER_CLASS, emitter_attributes
        # Attrib::RefSpec holds the class key, then the record key (the reverb preset).
        ref = {'type': 'Attrib::RefSpec', 'data': '204CAC1FD77088B8BEEFC8E3DE04FBAE0000000000000000'}
        kind = {'type': 'Sk8::Audio::eVolumeType', 'data': struct.pack('>i', 5).hex().upper()}
        collections = [{'class': EMITTER_CLASS, 'key': 'Hash_1F94F2F815C00368', 'parent': '',
                        'fields': {'Hash_99FD793BC30CF0FA': ref, 'Hash_6D18B8674D7E5337': kind}}]
        zone = emitter_attributes(collections)[0x1F94F2F815C00368]
        self.assertEqual(zone, {'reverb': 0xBEEFC8E3DE04FBAE, 'kind': 5})


class AemsFiles(unittest.TestCase):
    def test_copies_projects_in_archive_order_and_only_abkc_banks(self):
        import tempfile
        from pathlib import Path
        from types import SimpleNamespace
        from tools.asset_pipeline.audio_export import aems_files

        blobs = {
            'data/audio/b.csi': b'MOIR-b', 'data/audio/a.csi': b'MOIR-a',
            'data/audio/water.abk': b'ABKC-water', 'data/audio/emitter_utility.abk': b'ABKC-util',
            'data/audio/odd.abk': b'XXXX', 'data/audio/sfx.bnk': b'SPLC',
        }
        entries = [SimpleNamespace(path=path) for path in blobs]
        archive = SimpleNamespace(entries=entries, read=lambda e: blobs[e.path])
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            listed = aems_files(archive, out, ['water.abk', 'odd.abk', 'sfx.bnk', 'missing.abk'])
            self.assertEqual(listed['projects'], ['aems/b.csi', 'aems/a.csi'])
            self.assertEqual(listed['banks'], {'emitter_utility': 'aems/emitter_utility.abk', 'water': 'aems/water.abk'})
            self.assertEqual((out/'aems/water.abk').read_bytes(), b'ABKC-water')
            self.assertFalse((out/'aems/odd.abk').exists())


class MixMapFile(unittest.TestCase):
    def test_copies_the_mixmap_and_rejects_other_files(self):
        import tempfile
        from pathlib import Path
        from tools.asset_pipeline.audio_export import mixmap_file

        with tempfile.TemporaryDirectory() as tmp:
            root, out = Path(tmp)/'audio', Path(tmp)/'out'
            root.mkdir()
            self.assertIsNone(mixmap_file(root, out))
            data = struct.pack('>IIIi', 0, 2, 16, -1) + struct.pack('>ii', 24, -1) + b'\0' * 32
            (root/'MixMapSK8.mxb').write_bytes(data)
            self.assertEqual(mixmap_file(root, out), 'aems/MixMapSK8.mxb')
            self.assertEqual((out/'aems/MixMapSK8.mxb').read_bytes(), data)
            (root/'MixMapSK8.mxb').write_bytes(struct.pack('>IIIi', 0, 900, 16, -1))
            with self.assertRaises(ValueError):
                mixmap_file(root, out)


class GrainTuning(unittest.TestCase):
    def test_members_inherit_from_default_and_keep_exact_floats(self):
        from tools.asset_pipeline.audio_export import (GRAIN_CLASS, GRAIN_CURVE_FIELD, GRAIN_OWNER_CLASS,
                                                        GRAIN_PARAMS_FIELD, SURFACE_MAP, grain_tuning)

        hexf = lambda *v: struct.pack(f'>{len(v)}f', *v).hex().upper()
        f32 = lambda v: {'type': 'EA::Reflection::Float', 'data': hexf(v)}
        text = lambda v: {'type': 'EA::Reflection::Text', 'data': v}
        curve = [1, 1, 0, 0, 0.47, 0.7069, 0, 0, 0.25, 0.3621, 0, 0, 0, 0, 0, 0]
        collections = [
            {'class': GRAIN_CLASS, 'key': 'default', 'parent': '', 'fields': {
                'Hash_4890392C91829954': f32(74.0), 'Hash_5C9AA28695C17004': f32(0.8),
                GRAIN_CURVE_FIELD: {'type': 'EA::Reflection::Matrix44', 'data': hexf(*curve)},
                GRAIN_PARAMS_FIELD: {'type': 'GrainParams', 'array': {'items': [hexf(0.1, 0.2, 0.1, 1.6, 0.05),
                                                                                hexf(0.2, 0.1, 0.2, 1.5, 0.05)]}}}},
            {'class': GRAIN_CLASS, 'key': 'Hash_03721D0FA99A03C8', 'parent': 'default', 'fields': {
                'Hash_2C073BF8BC45063B': text('concrete_rough_hard.grain'), 'Hash_4890392C91829954': f32(60.0)}},
            {'class': GRAIN_OWNER_CLASS, 'key': 'default', 'parent': '', 'fields': {
                'Hash_7508154FF73DDCED': f32(35.0),
                'Hash_9BC13FA19CC4DF00': {'type': 'EA::Reflection::Int32', 'data': '000055F0'},
                GRAIN_PARAMS_FIELD: {'type': 'GrainParams', 'data': hexf(0.1, 0.4, 0.1, 2.4, 0.1)}}},
            {'class': SURFACE_MAP[0], 'key': SURFACE_MAP[1], 'parent': '', 'fields': {
                SURFACE_MAP[2]: {'type': 'Sk8::AudioSurfaceMap', 'array': {'items': ['00000000 00000003', '00000001 00000001']}}}},
        ]
        t = grain_tuning(collections)
        member = t['surfaces']['concrete_rough_hard']
        self.assertEqual(member['max_kmh'], 60.0)
        self.assertEqual(member['turn_cap'], struct.unpack('>f', struct.pack('>f', 0.8))[0])
        self.assertEqual(member['bezier'], [0.0, struct.unpack('>f', struct.pack('>f', 0.3621))[0],
                                            struct.unpack('>f', struct.pack('>f', 0.7069))[0], 1.0])
        self.assertEqual(len(member['params']), 2)
        self.assertEqual(t['default']['max_kmh'], 74.0)
        self.assertEqual(t['owner']['rocket_gain_word'], 22000)
        self.assertEqual(t['owner']['rocket_start_kmh'], 35.0)
        self.assertAlmostEqual(t['owner']['rocket_params'][3], 2.4, places=6)
        self.assertEqual(t['surface_map'], [3, 1])


class PlayerTuning(unittest.TestCase):
    def test_jitter_leaves_seams_grind_and_surface_rows(self):
        from tools.asset_pipeline.audio_export import (GRIND_CLASS, GRIND_SURFACE_KEYS, JITTER_CLASS, SEAM_CLASS,
                                                        SURFACE_MAP, player_tuning)
        from tools.asset_pipeline.vlt import hash64

        hexf = lambda *v: struct.pack(f'>{len(v)}f', *v).hex().upper()
        f32 = lambda v: {'type': 'EA::Reflection::Float', 'data': hexf(v)}
        i32 = lambda v: {'type': 'EA::Reflection::Int32', 'data': '%08X' % v}
        on = {'type': 'EA::Reflection::Bool', 'data': '01000000'}
        spider = 'Hash_%016X' % hash64('spidercrack')
        collections = [
            {'class': JITTER_CLASS, 'key': 'default', 'parent': '', 'fields': {
                'Hash_B66AAD957873A8B3': {'type': 'Sk8::Audio::eJitterParams', 'data': hexf(0, 0, 0, 0)}}},
            {'class': JITTER_CLASS, 'key': 'Hash_B355C0AD39002103', 'parent': 'default', 'fields': {
                'Hash_8F956FBAD301AE26': on}},
            {'class': JITTER_CLASS, 'key': 'Hash_02D9546BE518D5A1', 'parent': 'Hash_B355C0AD39002103', 'fields': {
                'Hash_B66AAD957873A8B3': {'type': 'Sk8::Audio::eJitterParams', 'data': hexf(16384, 16383, 100, 1)},
                'Hash_E7D491E2EB228F54': i32(4)}},
            {'class': SEAM_CLASS, 'key': 'default', 'parent': '', 'fields': {
                'Hash_FA3A57801765A2F8': f32(1.0), 'Hash_32A9692F1B826274': f32(1.0),
                'Hash_F713CB547B1DF920': i32(0), 'Hash_0608B3129FF81F12': i32(0)}},
            {'class': SEAM_CLASS, 'key': spider, 'parent': 'default', 'fields': {
                'Hash_FA3A57801765A2F8': f32(0.75), 'Hash_F713CB547B1DF920': i32(30),
                'Hash_CA81764BF5A85E34': {'type': 'Sk8::Audio::eSurfacePatternStyle', 'data': '00000002'},
                'Hash_107A78BA11A2B813': f32(3.0), 'Hash_D18F436B5764F260': i32(30)}},
            {'class': GRIND_CLASS, 'key': 'Hash_%016X' % GRIND_SURFACE_KEYS[0], 'parent': '', 'fields': {
                'Hash_0ECECDAC28B2B979': f32(0.0), 'Hash_C21983A2160ED3AC': f32(0.5)}},
            {'class': SURFACE_MAP[0], 'key': SURFACE_MAP[1], 'parent': '', 'fields': {
                SURFACE_MAP[2]: {'type': 'Sk8::AudioSurfaceMap', 'array': {'items': ['00000000 00000003 00000000 00000002 00000001']}}}},
        ]
        t = player_tuning(collections)
        # Leaves only (the two parents are not channels), fields through the parent chain.
        self.assertEqual(t['jitter'], [{'enabled': True, 'id': 4, 'params': [16384.0, 16383.0, 100.0, 1.0],
                                        'key': '02D9546BE518D5A1'}])
        self.assertEqual(len(t['seam_wobbles']), 16)
        self.assertEqual(t['seam_wobbles'][1], {'gain_low': 0.75, 'gain_high': 1.0, 'ms_low': 30, 'ms_high': 0,
                                                'mode': 2, 'grid_x': 3.0, 'angle': 30})
        self.assertEqual(t['seam_wobbles'][2], {}, 'no collection: retail reads the zero block')
        self.assertEqual(t['grind'][0], {'v': [0.0, 1.0, 1.0, 1.0], 'f': [1.0, 1.0, 0.5, 1.0]})
        self.assertEqual(t['grind'][1], {'v': [1.0] * 4, 'f': [1.0] * 4})
        self.assertEqual(t['surface_table'], [[0, 3, 0, 2, 1] + [0] * 13])
        self.assertNotIn('landing_materials', t)  # needs the TU3 image

    def test_bus_tuning_reverb_presets_by_offset_and_eq_bus_ranges(self):
        from tools.asset_pipeline.audio_export import (EQ_BUS_CLASS, EQ_BUS_KEYS, EQ_BUS_RANGES, REVERB_CLASS,
                                                        REVERB_FIELDS, bus_tuning)
        hexf = lambda *v: struct.pack(f'>{len(v)}f', *v).hex().upper()
        f32 = lambda v: {'type': 'EA::Reflection::Float', 'data': hexf(v)}
        collections = [
            {'class': REVERB_CLASS, 'key': 'default', 'parent': '', 'fields': {
                'Hash_' + REVERB_FIELDS[5]: f32(1.5), 'Hash_' + REVERB_FIELDS[12]: {'type': 'EA::Reflection::Int32', 'data': '00000000'}}},
            {'class': REVERB_CLASS, 'key': 'Hash_A2782D75A971CC8C', 'parent': 'default', 'fields': {
                'Hash_' + REVERB_FIELDS[6]: f32(70.0), 'Hash_' + REVERB_FIELDS[12]: {'type': 'EA::Reflection::Int32', 'data': '00000001'}}},
            {'class': EQ_BUS_CLASS, 'key': 'default', 'parent': '', 'fields': {
                'Hash_2086A0CE99C39A86': f32(100.0), 'Hash_C6DA68C12A3822D2': {'type': 'Bool', 'data': '00'}}},
            {'class': EQ_BUS_CLASS, 'key': 'Hash_' + EQ_BUS_KEYS[0], 'parent': 'default', 'fields': {
                'Hash_C6DA68C12A3822D2': {'type': 'Bool', 'data': '01'},
                'Hash_' + EQ_BUS_RANGES[0][0]: f32(450.0), 'Hash_' + EQ_BUS_RANGES[0][1]: f32(150.0)}},
        ]
        b = bus_tuning(collections)
        r = b['reverb']['A2782D75A971CC8C']
        self.assertEqual(len(r), 44)
        self.assertEqual((r[5], r[6], r[12]), (1.5, 70.0, 1), 'inherited T60, own size, int preset number')
        self.assertNotIn('default', b['reverb'])
        self.assertEqual(len(b['eq_buses']), 8)
        self.assertEqual(b['eq_buses'][0]['ranges'][0], [450.0, 150.0])
        self.assertTrue(b['eq_buses'][0]['enabled'])
        self.assertFalse(b['eq_buses'][1]['enabled'], 'a missing record reads default')
        self.assertEqual(b['eq_buses'][1]['clip'], 100.0)

    def test_audio_tricks_resolve_through_parents_by_name_hash(self):
        from tools.asset_pipeline.audio_export import TRICK_CLASS, TRICK_FIELD, player_tuning
        from tools.asset_pipeline.vlt import hash64

        trick = lambda v: {'type': 'Sk8::Audio::eSk8AudioTricks', 'data': '%08X' % (v & 0xFFFFFFFF)}
        ollie, base = 'Hash_%016X' % hash64('ollie'), 'Hash_%016X' % hash64('basetrick')
        t = player_tuning([
            {'class': TRICK_CLASS, 'key': base, 'parent': '', 'fields': {TRICK_FIELD: trick(-1)}},
            {'class': TRICK_CLASS, 'key': ollie, 'parent': base, 'fields': {TRICK_FIELD: trick(28)}},
            {'class': TRICK_CLASS, 'key': 'Hash_0000000000000001', 'parent': base, 'fields': {}},
        ])
        self.assertEqual(t['audio_tricks'], {ollie[5:]: 28, base[5:]: -1, '0000000000000001': -1})
        self.assertEqual(t['audio_tricks_2'], {}, 'no second field in these records')

    def test_collision_materials_follow_the_image_keys_and_refspecs(self):
        from tools.asset_pipeline.audio_export import (BAND_CLASS, MATERIAL_CLASS, MATERIAL_KEYS, MATERIAL_KINDS,
                                                        WINDOW_CLASS, player_tuning)

        hexf = lambda *v: struct.pack(f'>{len(v)}f', *v).hex().upper()
        i32 = lambda v: {'type': 'EA::Reflection::Int32', 'data': '%08X' % (v & 0xFFFFFFFF)}
        sc = lambda v: {'type': 'Skate_Collisions', 'data': '%08X' % v}
        ref = lambda cls, key: {'type': 'Attrib::RefSpec', 'data': cls[5:] + '%016X' % key + '0' * 16}
        board = 'Hash_%016X' % MATERIAL_KEYS[95]
        self.assertEqual(MATERIAL_KINDS[95], 0)
        self.assertEqual(MATERIAL_KINDS[8], 1)
        collections = [
            {'class': MATERIAL_CLASS, 'key': 'default', 'parent': '', 'fields': {
                'Hash_875BA75341DC8391': i32(32767), 'Hash_C090F2C1F048F17B': i32(4096),
                'Hash_3A3DD47E8DAFE796': i32(4096), 'Hash_1EBF9D2EB0DD56BA': {'type': 'EA::Reflection::Bool', 'data': '00'}}},
            {'class': MATERIAL_CLASS, 'key': board, 'parent': 'default', 'fields': {
                'Hash_875BA75341DC8391': i32(28000), 'Hash_9203DF6FD029B377': sc(993), 'Hash_BFABF634D2B1E45A': sc(876),
                'Hash_D5EF686287A57AFE': i32(6), 'Hash_82B1451A90152514': ref(WINDOW_CLASS, 7),
                'Hash_E228508FE0F53970': ref(BAND_CLASS, 8)}},
            {'class': WINDOW_CLASS, 'key': 'Hash_0000000000000007', 'parent': '', 'fields': {
                'Hash_036F313CEBC664FD': i32(10000), 'Hash_0D6EF57A39AF0C93': i32(22000),
                'Hash_1A5F7E8CCABBB0A2': {'type': 'EA::Reflection::Float', 'data': hexf(0.5)}}},
            {'class': BAND_CLASS, 'key': 'Hash_0000000000000008', 'parent': '', 'fields': {
                'Hash_C8DED1BC20B9D6A5': {'type': 'EA::Reflection::Float', 'data': hexf(0.4)},
                'Hash_D660AC459139BDF4': {'type': 'EA::Reflection::Float', 'data': hexf(0.025)}}},
            {'class': 'Hash_C26949FCB638A2CA', 'key': 'default', 'parent': '', 'fields': {
                'Hash_85FDC8BF696BCA5C': {'type': 'Sk8::Audio::eAudioMaterialTypes', 'data': '0000005F'},
                'Hash_733C45DF5B638ECB': {'type': 'sk82_cloth_foley', 'data': '00020002',
                                          'array': {'items': ['0000005F', '0000005E']}}}},
        ]
        t = player_tuning(collections)
        rows = t['collision']['materials']
        self.assertEqual(len(rows), 143)
        b = rows[95]
        self.assertEqual((b['kind'], b['gain'], b['pitch'], b['category']), (0, 28000, 4096, 6))
        self.assertEqual(b['ids'][:2], [993, 876], 'tier 2, tier 0 class 0 of the Skate_Collisions family')
        self.assertEqual(b['windows'][0], 10000)
        self.assertEqual(b['windows'][13], 22000, '+56 is the last window word')
        self.assertAlmostEqual(b['scale'], 0.5)
        self.assertAlmostEqual(b['bands'][0], 0.4, places=6)
        self.assertAlmostEqual(b['bands'][3], 0.025, places=6)
        self.assertNotIn('windows', rows[0], 'no record of its own: the zero record')
        self.assertEqual(rows[94]['kind'], -1)
        self.assertEqual(t['collision']['posters'], {'landing_board': 95, 'scuff_ids': [95, 94]})
        self.assertEqual(t['landing_materials'], [])

    def test_name_keyed_records_plant_ids_and_grind_contacts(self):
        from tools.asset_pipeline.audio_export import (BAND_CLASS, GRIND_CLASS, GRIND_CONTACTS, GRIND_METAL,
                                                        GRIND_SURFACE_KEYS, LIFT_FIELDS, MATERIAL_CLASS, PLANT_FIELDS,
                                                        player_tuning)
        from tools.asset_pipeline.vlt import hash64

        hexf = lambda v: {'type': 'EA::Reflection::Float', 'data': struct.pack('>f', v).hex().upper()}
        sc = lambda v: {'type': 'Skate_Collisions', 'data': '%08X' % v}
        foley = lambda v: {'type': 'sk82_cloth_foley', 'data': '%08X' % v}
        ref = lambda cls, key: {'type': 'Attrib::RefSpec', 'data': cls[5:] + '%016X' % key + '0' * 16}
        on = GRIND_CONTACTS['on']
        surface = 'Hash_%016X' % GRIND_SURFACE_KEYS[6]
        collections = [
            # The head's record is keyed by its name, its bands by the band class's `default`.
            {'class': MATERIAL_CLASS, 'key': 'head', 'parent': '', 'fields': {
                'Hash_9203DF6FD029B377': sc(1032), 'Hash_BFABF634D2B1E45A': sc(952),
                'Hash_E228508FE0F53970': ref(BAND_CLASS, hash64('default'))}},
            {'class': BAND_CLASS, 'key': 'default', 'parent': '', 'fields': {'Hash_C8DED1BC20B9D6A5': hexf(1.0)}},
            {'class': 'Hash_C26949FCB638A2CA', 'key': 'default', 'parent': '', 'fields': {
                **{f: foley(84 + i) for i, f in enumerate(PLANT_FIELDS)}, **{f: foley(90 + i) for i, f in enumerate(LIFT_FIELDS)}}},
            {'class': GRIND_CLASS, 'key': surface, 'parent': '', 'fields': {
                GRIND_METAL: {'type': 'EA::Reflection::Bool', 'data': '01'},
                **{f: {'type': 'Skate_Metal', 'data': '%08X' % (535 + i)} for i, f in enumerate(on['ids'][1])},
                on['gain'][0]: hexf(0.5), on['level'][0]: hexf(0.1), on['level'][1]: hexf(1.25),
                on['pitch'][0]: hexf(0.8), on['pitch'][1]: hexf(1.0)}},
        ]
        t = player_tuning(collections)
        head = t['collision']['materials'][97]
        self.assertNotIn('missing', head)
        self.assertEqual(head['ids'][:2], [1032, 952])
        self.assertAlmostEqual(head['bands'][0], 1.0)
        self.assertEqual(t['collision']['posters']['plant_ids'], [84, 85, 86, 87, 88])
        self.assertEqual(t['collision']['posters']['lift_ids'], [90, 91, 92, 93, 94])
        g = t['grind'][6]
        self.assertTrue(g['metal'])
        self.assertEqual(g['on']['ids'], [535, 536, 537, 538])
        self.assertAlmostEqual(g['on']['gain'][0], 0.5)
        self.assertEqual(g['on']['gain'][1], 1.0, 'a missing layer factor reads 1.0')
        self.assertNotIn('off', g, 'no off fields: no off sound')


class SpliceTrees(unittest.TestCase):
    def test_copies_the_patch_tree_of_splc_banks_only(self):
        import tempfile
        from pathlib import Path
        from types import SimpleNamespace
        from tools.asset_pipeline.audio_export import splice_trees

        # One record with one group of one member (sample 0), no containers, one sample; the
        # sample table starts right after the tree (offset from byte 60).
        member = struct.pack('>H', 0) + b'\0' * 70
        tree = struct.pack('>4s', b'SPLC') + b'\0' * 56
        record = b'\0' * 4 + struct.pack('>H', 0) + b'\0' + bytes([1]) + b'\0' * 28
        group = b'\0' * 8 + bytes([1, 0]) + b'\0' * 2
        body = record + group + member
        header = bytearray(tree)
        struct.pack_into('>IIIII', header, 8, len(body), 1, 0, 0, 1)
        bank = bytes(header) + body + b'SAMPLETABLE'
        blobs = {'data/audio/Skate_Collisions.bnk': bank, 'data/audio/water.abk': b'ABKC'}
        archive = SimpleNamespace(entries=[SimpleNamespace(path=p) for p in blobs], read=lambda e: blobs[e.path])
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            listed = splice_trees(archive, out, ['Skate_Collisions.bnk', 'water.abk', 'missing.bnk'])
            self.assertEqual(listed, {'Skate_Collisions': 'aems/Skate_Collisions.splc'})
            self.assertEqual((out/'aems/Skate_Collisions.splc').read_bytes(), bank[:60 + len(body)])
