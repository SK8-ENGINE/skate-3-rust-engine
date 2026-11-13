import math
import os
import struct
import unittest
from pathlib import Path

import numpy as np

from . import park_layout


def layout(static_format, static, dynamic=(), filler=16):
    """A `.park` file: header, block 1 allocated larger than its records (as retail saves it), block 2, empty 3 / 4."""
    stride = 36 if static_format == 1 else 48
    block1 = struct.pack('>2I', static_format, len(static))
    for position, rotation, piece, tint in static:
        block1 += struct.pack('>7fQ', *position, *rotation, piece)
        if static_format == 2:
            block1 += struct.pack('>3f', *tint)
    block1 += b'\x34' * (filler * len(static))
    block2 = struct.pack('>2I', 1, len(dynamic)) if dynamic else b''
    for position, rotation, piece in dynamic:
        block2 += struct.pack('>7fQI', *position, *rotation, piece, 1)
    block2 += b'\0' * (8 * len(dynamic))
    header = struct.pack('>5IQ', 12, len(block1), len(block2), 0, 0, 0x70366B8AEE681F34)
    return header + block1 + block2 + b'editor tail'


class ParkLayoutTest(unittest.TestCase):
    def test_static_records_skip_the_block_filler(self):
        quarter = (0.0, math.sqrt(0.5), 0.0, math.sqrt(0.5))
        raw = layout(2, [((1.0, 2.0, 3.0), quarter, 0x2C70170300030078, (0.5, 0.25, 1.0)),
                         ((4.0, 0.0, -5.0), (0.0, 1.0, 0.0, 0.0), 0x2C70170300030025, (1.0, 1.0, 1.0))],
                     dynamic=[((60.0, 1.5, -119.0), (0.0, 0.0, 0.0, 1.0), 0x2C70170400030001)])
        parsed = park_layout.parse(raw)
        self.assertEqual(parsed['version'], 12)
        self.assertEqual(parsed['origin_world_hash'], '70366B8AEE681F34')
        self.assertEqual([p['piece'] for p in parsed['static']], ['2C70170300030078', '2C70170300030025'])
        self.assertEqual(parsed['static'][0]['tint'], [0.5, 0.25, 1.0])
        self.assertEqual(parsed['static'][1]['position'], [4.0, 0.0, -5.0])
        self.assertEqual(len(parsed['dynamic']), 1)
        self.assertEqual(parsed['dynamic'][0]['piece'], '2C70170400030001')
        self.assertEqual(parsed['dynamic'][0]['flag'], 1)

    def test_the_editor_tail_holds_the_park_start(self):
        raw = layout(2, [((0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 1.0), 1, (1.0, 1.0, 1.0))], filler=16)
        raw = raw[:-len(b'editor tail')] + struct.pack('>16f', 0, 0, 1, 7, 0, 1, 0, 7, -1, 0, 0, 7, 241.25, 37.0, -91.0, 9e9)
        self.assertEqual(park_layout.parse(raw)['start'],
                         [[0.0, 0.0, 1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [241.25, 37.0, -91.0, 1.0]],
                         'w lanes are junk: rows get 0, the translation 1')
        self.assertIsNone(park_layout.parse(layout(2, []))['start'], 'a short tail has no start')

    def test_tutorial_records_have_no_tint(self):
        raw = layout(1, [((0.0, -0.05, 0.0), (0.0, 0.0, 0.0, 1.0), 0x1DC3303E38703, None)], filler=12)
        piece, = park_layout.parse(raw)['static']
        self.assertEqual(piece['tint'], [1.0, 1.0, 1.0])
        self.assertEqual(piece['piece'], '0001DC3303E38703')

    def test_blocks_past_the_end_are_rejected(self):
        raw = layout(2, [((0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 1.0), 1, (1.0, 1.0, 1.0))])
        with self.assertRaises(ValueError):
            park_layout.parse(raw[:60])

    def test_rotation_is_xyzw_and_row_vector(self):
        # A quarter turn about +Y (x y z w) takes +X to -Z, as Mat3::from_rotation_y does.
        m = park_layout.matrix([1.0, 2.0, 3.0], [0.0, math.sqrt(0.5), 0.0, math.sqrt(0.5)])
        np.testing.assert_allclose(np.array([1.0, 0.0, 0.0, 1.0]) @ m, [1.0, 2.0, 2.0, 1.0], atol=1e-6)
        np.testing.assert_allclose(np.array([0.0, 0.0, 0.0, 1.0]) @ m, [1.0, 2.0, 3.0, 1.0], atol=1e-6)

    def test_recipe_rows_name_their_model_and_caps(self):
        xml = (b'<?xml version="1.0" standalone="yes" ?>\n<compositeasset n="static_db" type="Static">'
               b'<mod id="0x2c70170300030078" n="da_minirampkicker_University"><ud n="cap">'
               b'<cap.AllowCollision value="" /><cap.Unlock value="skatepark_unlock_vert" /></ud>'
               b'<lod idx="0" id="0xf5761ab94e6a6bc1" arenaid="0x2c70170700030078"><matinst /></lod></mod>'
               b'</compositeasset>')
        row = park_layout.recipe(xml)['2C70170300030078']
        self.assertEqual(row['arena'], '2C70170700030078')
        self.assertEqual(row['name'], 'da_minirampkicker_University')
        self.assertEqual(row['caps'], {'AllowCollision': '', 'Unlock': 'skatepark_unlock_vert'})

    def test_teleporters_pair_the_first_start_and_end_of_a_channel(self):
        class NoModels:
            static, models = {}, {}
        piece = lambda index, key, position: dict(index=index, piece=key, position=position, rotation=[0.0, 0.0, 0.0, 1.0],
                                                  tint=[1.0, 1.0, 1.0])
        parsed = dict(static=[piece(0, 'S0', [1.0, 0.0, 2.0]), piece(1, 'E0', [5.0, 3.0, 6.0]),
                              piece(2, 'S0', [9.0, 9.0, 9.0]), piece(3, 'S1', [0.0, 0.0, 0.0])], dynamic=[])
        roles = {'S0': (0, 0), 'E0': (1, 0), 'S1': (0, 1)}
        found = park_layout.layout_teleporters(NoModels(), 'ZenPlaza_Full', 'Dist_Zen', parsed, roles)
        self.assertEqual(len(found), 1, 'channel 1 has no end piece')
        t = found[0]
        self.assertEqual((t['id'], t['map'], t['layout'], t['title_id']), ('skpk:ZenPlaza_Full:0', 'Zen', 'ZenPlaza_Full', 'ID_COMMON_TELEPORT'))
        self.assertEqual(t['volume']['shape']['center'], [1.0, 0.0, 2.0], 'the first start piece, not the second')
        self.assertEqual(t['volume']['shape']['half_extents'], [2.0, 2.0, 2.0])
        self.assertEqual(t['exits'][0]['matrix'][3], [5.0, 3.0, 6.0, 1.0])

    def test_park_shaders_have_their_own_families(self):
        from .retail_material import _retail_shader_family
        self.assertEqual([_retail_shader_family('environmentpark.' + n) for n in ('diffuse', 'decal', 'alphatest')],
                         [50, 51, 52])
        self.assertEqual(_retail_shader_family('environmentsimple.alphatest'), 7, 'neighbours unchanged')

    def test_shadow_colours_only_for_park_worlds(self):
        from .environment import key_hash
        vec = lambda *v: dict(data=struct.pack('>4f', *v, 0.0).hex().upper())
        park = {key_hash(park_layout.PARK_WORLD_FIELD): dict(data='01000000'),
                key_hash(park_layout.DYNAMIC_SHADOW_FIELD): vec(0.5, 0.25, 0.125),
                key_hash(park_layout.EXPONENTIAL_SHADOW_FIELD): vec(1.0, 0.5, 0.75)}
        self.assertEqual(park_layout.shadow_colours(park), dict(dynamic=[0.5, 0.25, 0.125], exponential=[1.0, 0.5, 0.75]))
        city = {**park, key_hash(park_layout.PARK_WORLD_FIELD): dict(data='00000000')}
        self.assertIsNone(park_layout.shadow_colours(city))

    def test_projected_lightmap_reads_its_name_and_bounds(self):
        from .environment import key_hash
        f = lambda v: dict(data=struct.pack('>f', v).hex().upper())
        fields = {key_hash(park_layout.LIGHTMAP_NAME_FIELD): dict(data='megaparklightmap'),
                  **{key_hash(name): f(v) for name, v in zip(park_layout.LIGHTMAP_BOUNDS_FIELDS[:3], (25.5, 254.5, -39.0))}}
        found = park_layout.projected_lightmap(fields)
        self.assertEqual(found, dict(texture='megaparklightmap', bounds=[25.5, 254.5, -39.0, 0.0]), 'a missing field reads 0')
        self.assertIsNone(park_layout.projected_lightmap({key_hash(park_layout.LIGHTMAP_NAME_FIELD): dict(data='')}))

    @unittest.skipUnless(os.environ.get('SKATE3_DISC'), 'needs the extracted disc (SKATE3_DISC)')
    def test_disc_layouts_resolve_every_piece(self):
        from tools.owned_game.big import BigArchive
        disc = Path(os.environ['SKATE3_DISC'])
        sources = park_layout.Sources([BigArchive(disc/'data/big/miscload.big'),
                                       BigArchive(disc/'data/content/parkassets.big')])
        counts = {}
        for name in ('MegaPark_Fullpark', 'Industrial_Fullpark', 'Starter_Fullpark', 'startpark_progression',
                     'Downtown_Fullpark'):
            parsed = park_layout.parse(sources.read(sources.layouts, sources.layout(f'skatepark\\{name}.park')))
            counts[name] = len(parsed['static'])
            self.assertFalse([p for p in parsed['static'] if p['piece'] not in sources.static], name)
        # Every MegaPark piece has its retail ClusteredMesh, the rails and fences through the 20-byte link form (b437).
        from .dynamic_props import records, sections, template_collision
        for key in {p['piece'] for p in park_layout.parse(sources.read(sources.layouts, sources.layout('skatepark\\MegaPark_Fullpark.park')))['static']}:
            raw = sources.read(sources.models, sources.static[key]['arena'])
            (_, at, _), = records(raw, 0xEB000D, 160)
            self.assertIsNotNone(template_collision(raw, sections(raw), at), sources.static[key]['name'])
        # b424 / b425 counts.
        self.assertEqual(counts, {'MegaPark_Fullpark': 450, 'Industrial_Fullpark': 328, 'Starter_Fullpark': 256,
                                  'startpark_progression': 251, 'Downtown_Fullpark': 238})


if __name__ == '__main__':
    unittest.main()
