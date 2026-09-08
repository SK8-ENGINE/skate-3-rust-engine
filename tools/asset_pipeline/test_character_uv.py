"""Regressions for the RX2 hair float UV declaration and glTF layout."""
import struct
import unittest

from tools.asset_pipeline.character_glb import Glb, RX2


class CharacterUvTests(unittest.TestCase):
    def test_float_uv_preserves_signed_values_and_stride(self):
        fmt, size = RX2.VTX_FORMATS[0x002C23A5]
        self.assertEqual((fmt, size), ('FLOAT2', 8))
        source = b'head' + struct.pack('>I2fI2f', 0, -0.25, 1.5, 0, 0.125, 0.75)
        descriptor = {'stride': 12, 'elements': [
            {'usage_name': 'TEXCOORD', 'usage_index': 1,
             'format_name': fmt, 'offset': 4}]}
        uv = RX2._decode_vertices(source, 4, 2, descriptor)['uvs2']
        self.assertEqual(uv, [(-0.25, 1.5), (0.125, 0.75)])
        glb = Glb()
        index = glb.accessor(uv, 'VEC2')
        accessor = glb.doc['accessors'][index]
        self.assertEqual(accessor['count'], 2)
        self.assertEqual(glb.doc['bufferViews'][accessor['bufferView']]['byteLength'], 16)
        self.assertEqual(struct.unpack('<4f', glb.data), (-0.25, 1.5, 0.125, 0.75))

    def test_accessor_rejects_wrong_component_count(self):
        with self.assertRaisesRegex(ValueError, 'shape'):
            Glb().accessor([(1, 2, 3, 4)], 'VEC2')


if __name__ == '__main__':
    unittest.main()
