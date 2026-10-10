"""Global resource GUID selection without owned-game fixtures."""
import struct
import unittest
from types import SimpleNamespace

from .backdrop import presentation_meshes, proxy_drawn_files, proxy_texture_keys, stream_cell, texture_groups
from .sky import _texture


class BackdropTests(unittest.TestCase):
    def test_missing_channels_do_not_shift_later_materials(self):
        # Ocean has no diffuse here; foliage follows it. Name text and GUIDs
        # deliberately differ, as they do in the global University resource.
        parameters = [('Name', 0), ('normal', 19), ('Name', 0),
                      ('diffuse', 0x123456789ABCDEF0), ('lightmap', 23)]
        raw = bytearray(512)
        struct.pack_into('>8I', raw, 0, 0, len(parameters), 0, 32,
                         32 + len(parameters)*32, 0, 0, 0)
        at = 256
        for i, (kind, guid) in enumerate(parameters):
            word = kind.encode() + b'\0'
            raw[at:at+len(word)] = word
            struct.pack_into('>8I', raw, 32+i*32, at, 0, 0, 0,
                             guid >> 32, guid & 0xffffffff, 0, 0)
            at += len(word)
        table = SimpleNamespace(entries=[SimpleNamespace(type_id=0xeb0005, f0=0)])
        self.assertEqual(texture_groups(bytes(raw), table, ('diffuse', 'normal', 'lightmap')),
                         [{'normal': 19}, {'diffuse': 0x123456789ABCDEF0, 'lightmap': 23}])

    def test_texture_guid_uses_handle_not_list_position(self):
        raw = bytearray(64)
        struct.pack_into('>2I', raw, 0, 1, 8)
        struct.pack_into('>6I', raw, 8, 0, 0, 0x12345678, 0x9abcdef0, 0, 27)
        wanted = SimpleNamespace(index=27)
        textures = SimpleNamespace(data=bytes(raw),
            entries=[SimpleNamespace(type_id=0xeb000b, f0=0)],
            textures=[SimpleNamespace(index=1), wanted])
        self.assertIs(_texture(textures, 0x123456789ABCDEF0), wanted)
        with self.assertRaises(ValueError):
            _texture(textures, 123)


    def test_every_global_mesh_is_kept(self):
        # Industrial's sea surface is ocean.default; distant shore is
        # environment.default. Retail draws the whole model.
        shaders = ['environment.default', 'ocean.default', 'ocean.reflection',
                   'environmentsimple.alphatest', 'tree.default', 'environment.reflective_simple']
        metadata = [{'shader_name': s} for s in shaders]
        self.assertEqual([i for i, _ in presentation_meshes(metadata)], list(range(len(shaders))))

    def test_proxy_cell_draws_only_without_full_detail_partner(self):
        # Retail deactivates cPres_X_Z_high_proxy while cPres_X_Z_high is
        # active; the engine keeps every full cell, so only unpaired ones draw.
        district = ['cPres_1_2_high.xsf', 'cPres_-3_4_high.xsf', 'cPres_Global.xsf']
        proxy = ['cPres_1_2_high_proxy.xsf', 'cPres_-3_4_high_proxy.xsf', 'cPres_-3_5_high_proxy.xsf',
                 'cPres_Global_proxy.xsf']
        self.assertEqual(proxy_drawn_files(proxy, district), ['cPres_-3_5_high_proxy.xsf', 'cPres_Global_proxy.xsf'])
        self.assertEqual(stream_cell('cPres_-12_7_high_proxy.xsf'), (-12, 7))
        self.assertIsNone(stream_cell('cPres_Global_proxy.xsf'))

    def test_proxy_texture_ids_drop_the_tex_table_flag(self):
        models = [{'meshes': [{'texture_id': '0x2c70170a00170f9c',
                               'retail_texture_ids': {'diffuse': '0x2c70170a00170f9c', 'noise': '0x0000809703e3870a'}}]}]
        textures = {'0xac70170a00170f9c': 'a', '0x0000809703e3870a': 'n', '0xac70170a00170000': 'unused'}
        self.assertEqual(proxy_texture_keys(textures, models), {'0x2c70170a00170f9c': 'a', '0x0000809703e3870a': 'n'})

    def test_proxy_cells_per_district_on_owned_disc(self):
        # Industrial's south hills: 59 unpaired cells; DownTown/University none.
        import os
        from pathlib import Path
        from tools.owned_game.big import BigArchive
        disc = os.environ.get('SKATE3_DISC')
        if not disc:
            self.skipTest('SKATE3_DISC not set')
        content = Path(disc)/'data/content'
        names = lambda big: [Path(e.path).name for e in BigArchive(content/big).entries
                             if Path(e.path).name.startswith('cPres_')]
        for proxy, district, cells in (('Industrial', 'Industrial', 59), ('Downtown', 'DownTown', 0),
                                       ('University', 'University', 0)):
            drawn = proxy_drawn_files(names(f'proxy{proxy}_100_Proxy.big'), names(f'worldDIST_{district}.big'))
            self.assertEqual(sum(stream_cell(n) is not None for n in drawn), cells, proxy)


if __name__ == '__main__':
    unittest.main()
