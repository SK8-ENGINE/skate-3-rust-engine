import unittest

from tools.asset_pipeline import audio_export
from tools.asset_pipeline.audio_formats import name_id


def field(kind, data, items=None, size=None):
    f = {'type': kind, 'data': data}
    if items is not None:
        f['array'] = {'capacity': len(items), 'element_size': size, 'alignment': 4, 'items': items}
    return f


class MusicZones(unittest.TestCase):
    def test_playlists_parse_songs_and_pieces_through_parents(self):
        parent = {'class': audio_export.PLAYLIST_CLASS, 'key': 'Hash_00000000000000AA', 'parent': '', 'fields': {
            'Hash_99B1389B44C64809': field('Sk8::Audio::ePlayListMode', '00000001'),
            'Hash_FC6C57A267A28231': field('Sk8::Audio::eChannelMapPreset', '00000003')}}
        child = {'class': audio_export.PLAYLIST_CLASS, 'key': 'Hash_00000000000000BB', 'parent': 'Hash_00000000000000AA', 'fields': {
            'Hash_F41187C8E3599333': field('Sk8::Audio::DJSongInfo', '', ['9FF750AB3FE66666434300000000009B'], 16)}}
        licensed = {'class': audio_export.PLAYLIST_CLASS, 'key': 'Hash_00000000000000CC', 'parent': '', 'fields': {
            'Hash_FC6C57A267A28231': field('Sk8::Audio::eChannelMapPreset', '00000004'),
            'Hash_E7CC11BB107155EC': field('Sk8::Audio::IPodSongInfo', '', ['4CAB08CC3F66666600000076'], 12)}}
        out = audio_export.music_playlists([parent, child, licensed])
        self.assertEqual(out['00000000000000BB'], {'mode': 1, 'preset': 3, 'pieces': [[0x9FF750AB, 1.8, 195.0, 155]]})
        self.assertEqual(out['00000000000000CC'], {'preset': 4, 'songs': [[0x4CAB08CC, 0.9, 118]]})

    def test_interactive_records_by_name(self):
        default = {'class': audio_export.INTERACTIVE_CLASS, 'key': 'default', 'parent': '', 'fields': {
            'Hash_6AAE7EF9E3C13ABD': field('EA::Reflection::Float', '41F00000')}}
        tier = {'class': audio_export.INTERACTIVE_CLASS, 'key': f'Hash_{name_id("times2"):016X}', 'parent': 'default', 'fields': {
            'Hash_D56BFF3C91AAD6E0': field('EA::Reflection::Int32', '00000070')}}
        out = audio_export.music_interactive([default, tier])
        self.assertEqual(out, {'default': {'speed_gate': 30.0}, 'times2': {'speed_gate': 30.0, 'value_min': 112}})

    def test_music_settings_keep_plain_values_and_raw_graph_types(self):
        record = {'class': audio_export.MUSIC_CLASS, 'key': 'Hash_C37B3862A94AFF2E', 'parent': '', 'fields': {
            'Hash_8A5B99BA28348132': field('EA::Reflection::Text', 'Chase_Scratch'),
            'Hash_0B91E756EA46FB6C': field('EA::Reflection::Float', '3FE00000'),
            'Hash_2834CACED3DB5A4E': {'type': 'EA::Reflection::Text', 'data': '', 'array': {'items': ['0000310E'], 'text_items': ['Cymbal_Overlay']}},
            'Hash_916CB408916CB408': field('Sk8::Audio::PointGraphData8', '000000003E87EBFC')}}
        out = audio_export.music_settings([record])
        self.assertEqual(out['C37B3862A94AFF2E']['fields'], {
            '8A5B99BA28348132': 'Chase_Scratch', '0B91E756EA46FB6C': 1.75, '2834CACED3DB5A4E': ['Cymbal_Overlay'],
            '916CB408916CB408': {'type': 'Sk8::Audio::PointGraphData8', 'data': '000000003E87EBFC'}})

    def test_race_music_reads_fields_and_the_curve(self):
        curve = ''.join(f'{v:08X}' for v in [0, 0x3F800000] + [0x3F800000] * 6 + [0, 0x3F000000] + [0x3F800000] * 6)
        record = {'class': audio_export.RACE_MUSIC_CLASS, 'key': 'default', 'parent': '', 'fields': {
            'Hash_1B241A8EEA6E0CD7': field('EA::Reflection::Float', '40A00000'),
            'Hash_929F268EC1F9354F': field('EA::Reflection::Int32', '0000007F'),
            'Hash_EAD4643494A5FF26': field('Sk8::PointGraphData8', curve)}}
        out = audio_export.race_music([record])['default']
        self.assertEqual((out['delay'], out['place1']), (5.0, 127))
        self.assertEqual((out['curve_x'][:2], out['curve_y'][:2]), ([0.0, 1.0], [0.0, 0.5]))


if __name__ == '__main__':
    unittest.main()
