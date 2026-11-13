import struct
import unittest

from .teleporters import CHALLENGE, indicator
from .environment import Collections
from .visual_indicators import COLOUR_FIELDS, PARK_WORLD, SPLINES, STYLE_FIELDS, colours, park_maps, parse_splines, styles


def arena(segments):
    """A minimal RW4 arena with one VISUALINDICATORDATA object holding one spline of `segments` 4x4 rows."""
    body = struct.pack('>4I', 1, len(segments), 16, 80)
    body += struct.pack('>QQ', 0x1234, 0x0001DA7E03E38707) + struct.pack('>4f', -1, 0, 0, 0) + struct.pack('>4f', 1, 2, 0, 0)
    body += struct.pack('>4I', 3, len(segments), 80, 0)
    for rows in segments:
        body += b''.join(struct.pack('>4f', *r, 1.0 if i == 3 else 0.0) for i, r in enumerate(rows))
    base = 0x40
    table = base + len(body)
    head = bytearray(b'\x89RW4xb2'.ljust(0x40, b'\0'))
    struct.pack_into('>I', head, 0x20, 1)
    struct.pack_into('>I', head, 0x30, table)
    return bytes(head) + body + struct.pack('>6I', base, 0, len(body), 16, 0x24, SPLINES)


class VisualIndicatorTests(unittest.TestCase):
    def test_splines_read_power_basis_segments(self):
        rows = [(0, 0.671, 0), (0, -1.007, 0), (0, 0, 0), (-0.487, 1.489, 0)]
        splines = parse_splines(arena([rows]))
        spline = splines['0001DA7E03E38707']
        self.assertEqual((spline['kind'], spline['name'], len(spline['segments'])), (3, '0000000000001234', 1))
        self.assertAlmostEqual(spline['segments'][0][3][1], 1.489, places=5)
        self.assertEqual(spline['max'], [1.0, 2.0, 0.0])

    def test_challenge_indicator_inherits_the_group_key_and_uses_its_own_locator(self):
        rows = {(CHALLENGE, 'tele_world_to_stadium'): ('teleports', {'locator': 'tele_world_to_stadium_origin_locator_01'}),
                (CHALLENGE, 'teleports'): ('main', {'indicator': 0x3A6EF9FB9B0381CF}),
                (CHALLENGE, 'main'): ('', {})}
        matrix = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [373.88, 64.18, -61.72, 1]]
        found = indicator(rows, 'tele_world_to_stadium', {0x3A6EF9FB9B0381CF: 'teleport'}, lambda name: [matrix])
        self.assertEqual(found, dict(key='teleport', locator='tele_world_to_stadium_origin_locator_01', matrix=matrix))
        self.assertIsNone(indicator(rows, 'teleports', {}, lambda name: [matrix]))  # the group has no locator
        self.assertIsInstance(indicator(rows, 'tele_world_to_stadium', {}, lambda name: []), str)

    def test_style_rows_give_the_draw_fields_with_inheritance(self):
        graph = struct.pack('>12f', 0, 0, 100, 1, 4.25, 13.5, 100, 100, 0, 1, 1, 0).hex()
        base = {STYLE_FIELDS['width_a']: struct.pack('>f', 0.2).hex(), STYLE_FIELDS['width_b']: struct.pack('>f', 0.15).hex(),
                STYLE_FIELDS['samples']: '000A', STYLE_FIELDS['passes']: '0001', STYLE_FIELDS['three_strips']: '01000000',
                STYLE_FIELDS['fade']: graph, STYLE_FIELDS['intensity_front']: struct.pack('>f', 1.0).hex(),
                STYLE_FIELDS['intensity_background']: struct.pack('>f', 0.75).hex()}
        rows = [dict({'class': 'visualindicators', 'key': 'default', 'parent': '', 'fields': {k: {'data': v} for k, v in base.items()}}),
                dict({'class': 'visualindicators', 'key': 'gates', 'parent': 'default', 'fields': {STYLE_FIELDS['samples']: {'data': '0004'}}})]
        found = styles(Collections({'collections': rows}))
        self.assertEqual(found['gates']['samples'], 4)
        self.assertTrue(found['gates']['three_strips'])
        self.assertAlmostEqual(found['default']['width'][1], 0.15, places=6)
        self.assertEqual(found['default']['fade'], [4.25, 13.5, 100.0, 100.0])
        self.assertEqual(found['default']['fade_alpha'], [0.0, 1.0, 1.0, 0.0])
        self.assertEqual(found['gates']['intensity'], [1.0, 0.75])

    def test_colour_rows_name_both_pass_textures_and_the_tint(self):
        text = lambda s: {'data': s}
        rows = [{'class': 'ribbon_spline_colour', 'key': 'default', 'parent': '',
                 'fields': {COLOUR_FIELDS['front']: text('visual_indicator_blue.Texture'),
                            COLOUR_FIELDS['background']: text('visual_indicator_bg_blue.Texture'),
                            COLOUR_FIELDS['tint']: text(struct.pack('>4f', 2, 2, 2, 0.5).hex())}},
                {'class': 'ribbon_spline_colour', 'key': 'red', 'parent': 'default',
                 'fields': {COLOUR_FIELDS['front']: text('visual_indicator_red.Texture')}}]
        found = colours(Collections({'collections': rows}))
        self.assertEqual(found['default'], dict(front='visual_indicator_blue', background='visual_indicator_bg_blue', tint=[2.0, 2.0, 2.0, 0.5]))
        self.assertEqual((found['red']['front'], found['red']['background']), ('visual_indicator_red', 'visual_indicator_bg_blue'))

    def test_park_maps_come_from_the_world_rows_park_bool(self):
        world = lambda key, parent, fields: {'class': 'world', 'key': key, 'parent': parent,
                                             'fields': {k: {'data': v} for k, v in fields.items()}}
        rows = [world('default_skateparks', '', {PARK_WORLD: '01'}),
                world('dist_zenplazapark_full', 'default_skateparks', {'WorldStream': 'Dist_Zen'}),
                world('dist_university', '', {'WorldStream': 'DIST_University', PARK_WORLD: '00'})]
        self.assertEqual(park_maps(Collections({'collections': rows})), ['Zen'])


if __name__ == '__main__':
    unittest.main()
