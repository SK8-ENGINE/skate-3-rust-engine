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


if __name__ == '__main__':
    unittest.main()
