import struct
import unittest
from unittest.mock import patch
from pathlib import Path
from tempfile import TemporaryDirectory
import numpy as np

import json
import os
import sys

from .dynamic_props import (locators, transform_mesh, template_meshes, save_catalog, load_catalog, mobj_extension,
                            characteristics_key, records)


def resource(kind, payload):
    raw = bytearray(88+len(payload))
    raw[:7] = b'\x89RW4xb2'
    struct.pack_into('>I', raw, 32, 1)
    struct.pack_into('>I', raw, 48, 64)
    struct.pack_into('>6I', raw, 64, 88, 0, len(payload), 0, 0, kind)
    raw[88:] = payload
    return raw


class DynamicPropsTests(unittest.TestCase):
    def test_transform_does_not_read_other_meshes(self):
        class LazyArrays(dict):
            def __getitem__(self, key):
                if key.endswith('_1'):
                    raise AssertionError('Unrelated mesh was decompressed')
                return super().__getitem__(key)
        arrays = LazyArrays(vertices_0=np.zeros((3, 3)), faces_0=np.array([[0, 1, 2]]), vertices_1=None)
        result = transform_mesh(arrays, 0, np.eye(4))
        np.testing.assert_array_equal(result['faces_0'], [[0, 1, 2]])

    def test_catalog_roundtrip_preserves_double_precision_and_bindings(self):
        template = dict(matrix=np.arange(16, dtype=float).reshape(4, 4)/7,
                        model_matrix=np.eye(4), npz=Path('model.npz'),
                        meshes=[dict(retail_texture_ids={'diffuse': '0xf123456789abcdef'})],
                        asset_id='original', mesh_info=[112])
        textures={'0xf123456789abcdef': dict(width=4, height=4, rgba='original.rgba')}
        with TemporaryDirectory() as work:
            path=Path(work)/'catalog.json'
            with patch('tools.asset_pipeline.dynamic_props.catalog', return_value=({'template':template}, textures)):
                save_catalog([], path)
            templates, actual_textures=load_catalog(path)
        actual=templates['template']
        for key in ('matrix','model_matrix'):
            self.assertEqual(actual[key].tobytes(),template[key].tobytes())
        self.assertEqual(actual['meshes'],template['meshes'])
        self.assertEqual(actual_textures,textures)

    def test_locator_id_and_row_matrix(self):
        payload = bytearray(166)
        struct.pack_into('>5I', payload, 0, 0, 1, 1, 32, 160)
        m = np.eye(4); m[3, :3] = [10, 20, -30]
        struct.pack_into('>16f', payload, 32, *m.ravel())
        struct.pack_into('>3Q2I', payload, 128, 123, 456, 789, 0, 160)
        payload[160:] = b'bench\0'
        rows = locators(resource(0xEB001D, payload))
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]['template_id'], '0000000000000315')
        self.assertEqual(rows[0]['matrix'][3], [10, 20, -30, 1])
        self.assertEqual(rows[0]['name'], 'bench')
        struct.pack_into('>I', payload, 16, 159)
        with self.assertRaisesRegex(ValueError, 'layout'):
            locators(resource(0xEB001D, payload))

    def test_scaled_rotation_and_normal_transform(self):
        m = np.array([[0, 0, -2, 0], [0, 3, 0, 0], [4, 0, 0, 0], [10, 20, 30, 1]], dtype=float)
        arrays = {'vertices_0': np.array([[1, 2, 3]]), 'normals_0': np.array([[0, 0, 1]]), 'faces_0': np.array([[0, 0, 0]])}
        transformed = transform_mesh(arrays, 0, m)
        np.testing.assert_allclose(transformed['vertices_0'], [[22, 26, 28]])
        np.testing.assert_allclose(transformed['normals_0'], [[1, 0, 0]])
        np.testing.assert_array_equal(arrays['vertices_0'], [[1, 2, 3]])

    def test_mobj_extension_encodes_shared_ranges_and_affines(self):
        rotation = np.array([[0, 0, -1, 0], [0, 1, 0, 0], [1, 0, 0, 0], [10, 20, 30, 1]], dtype=float)
        items = [dict(instance_id='0000000100000007', template_id='0000000000000042', name='ramp_a'),
                 dict(instance_id='0000000100000008', template_id='0000000000000042', name='ramp_b')]
        payload = mobj_extension([(items[0], 0, rotation), (items[1], 0, rotation)], [(12, 36)])
        self.assertEqual(struct.unpack_from('<I', payload, 0)[0], 2)
        at = 4
        for i, item in enumerate(items):
            identity, length = struct.unpack_from('<2I', payload, at); at += 8
            self.assertEqual(identity, 7+i)
            name = payload[at:at+length].decode(); at += length
            self.assertEqual(name, '0000000000000042/'+item['name'])
            origin = struct.unpack_from('<3f', payload, at); at += 12
            self.assertEqual(origin, (10., 20., 30.))
            first, count = struct.unpack_from('<2I', payload, at); at += 8
            self.assertEqual((first, count), (12, 36))
            at += 8+4  # collision range, no rails
            physics, shape = struct.unpack_from('<2I', payload, at); at += 8
            self.assertEqual((physics, shape), (0, 0))
            at += 24+8  # physics float defaults, boolean flags
            affine = struct.unpack_from('<12f', payload, at); at += 48
            np.testing.assert_allclose(affine, list(rotation[:3, :3].ravel())+[10, 20, 30])
        self.assertEqual(at, len(payload))
        duplicate = dict(items[1], instance_id=items[0]['instance_id'])
        with self.assertRaisesRegex(ValueError, 'identity'):
            mobj_extension([(items[0], 0, rotation), (duplicate, 0, rotation)], [(12, 36)])

    def test_missing_template_reference_is_rejected(self):
        payload = bytearray(192)
        struct.pack_into('>5I', payload, 0, 0, 1, 1, 32, 192)
        struct.pack_into('>I', payload, 160, 100)
        with self.assertRaisesRegex(ValueError, 'model reference'):
            template_meshes(resource(0xEB000D, payload))


# Fields of livingworld_dynamicobject_characteristics the DMO reads (layout
# offsets from the schema; doc 26 "Per-type DMO data").
TYPE_FIELDS = ('Hash_5CCD5998E03C299B', 'Hash_C4D8A03586A31915', 'Hash_E0101A9DFD63DEE9',
               'Hash_6E0BB4F5881A4841', 'Hash_CDA7A31C5EDBEB6E', 'Hash_086956BCA2187458')


class DmoTypeDataTests(unittest.TestCase):
    def test_characteristics_key_is_the_record_at_120(self):
        payload = bytearray(160)
        struct.pack_into('>Q', payload, 120, 0x028BDAC3B6F3A059)
        self.assertEqual(characteristics_key(payload, 0), 'Hash_028BDAC3B6F3A059')
        self.assertIsNone(characteristics_key(bytearray(160), 0))

    @unittest.skipUnless(os.environ.get('SKATE3_DISC'), 'set SKATE3_DISC (extracted disc) and SKATE3_ASSET_ROOT (set-up assets)')
    def test_every_disc_dmo_type_has_vault_type_data(self):
        from tools.owned_game.big import BigArchive
        disc = Path(os.environ['SKATE3_DISC'])
        assets = Path(os.environ.get('SKATE3_ASSET_ROOT', Path(__file__).resolve().parents[2]/'assets'))
        stock = assets/'private/stock/skater-collections.json'
        collections = json.loads(stock.read_text(encoding='utf-8'))['collections']
        vault = {c['key']: c for c in collections if c['class'] == 'livingworld_dynamicobject_characteristics'}
        from .vlt import hash64
        by_id = {f'Hash_{hash64(k):016X}': k for k in vault}
        def field(key, name):
            while key:
                if name in vault[key]['fields']:
                    return vault[key]['fields'][name]
                key = vault[key]['parent']
            return None
        sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/university/tools/vanilla_map_extraction/tools'))
        import skate3_streams
        templates = {}
        with TemporaryDirectory() as work:
            archive = BigArchive(disc/'data/content/worlddmo.big')
            archive.extract_entries(archive.entries, Path(work))
            for stream in (Path(work)/'data/content/world/dmo').iterdir():
                for asset in skate3_streams.load_global_stream(stream, 'Pres', stream.name):
                    if asset.record.asset_type != skate3_streams.ASSET_TYPE_MODEL:
                        continue
                    for _, at, _ in records(asset.data, 0xEB000D, 160):
                        tid = struct.unpack_from('>Q', asset.data, at+104)[0]
                        templates[tid] = characteristics_key(asset.data, at)
        self.assertGreater(len(templates), 100)
        for tid, key in templates.items():
            self.assertIn(key, by_id, f'template {tid:016X}')
            for name in TYPE_FIELDS:
                self.assertIsNotNone(field(by_id[key], name), f'{by_id[key]} {name}')


if __name__ == '__main__':
    unittest.main()
