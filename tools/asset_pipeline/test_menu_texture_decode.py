"""The menu movies' DXT decoder (vendor/skate3_ui/xenos.py): the block-at-once decoder equals the per-pixel one."""
from pathlib import Path
import random
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from vendor.skate3_ui import xenos  # noqa: E402
from vendor.skate3_ui import refpack  # noqa: E402


class MenuTextureDecodeTests(unittest.TestCase):
    def test_block_decoder_equals_the_per_pixel_one(self):
        rng = random.Random(7)
        for fmt in (18, 19, 20):
            block = 8 if fmt == 18 else 16
            for width, height in ((1, 1), (3, 5), (4, 4), (7, 9), (16, 8), (64, 64), (13, 128), (256, 4)):
                blocks = max(1, (width + 3) // 4) * max(1, (height + 3) // 4)
                for trial in range(20):
                    data = bytearray(rng.getrandbits(8) for _ in range(blocks * block + rng.randint(0, 7)))
                    if trial % 3 == 0:
                        # Three-colour blocks (c0 <= c1) and six-step alpha (a0 <= a1).
                        for k in range(blocks):
                            o = k * block + block - 8
                            data[o:o + 4] = bytes((1, 0, 2, 0))
                            if fmt == 20:
                                data[k * block:k * block + 2] = bytes((10, 200))
                    with self.subTest(fmt=fmt, size=(width, height), trial=trial):
                        self.assertEqual(xenos._decode_dxt(width, height, bytes(data), fmt),
                                         xenos._decode_dxt_reference(width, height, bytes(data), fmt))

    def test_a_truncated_base_level_is_refused_by_both(self):
        for decode in (xenos._decode_dxt, xenos._decode_dxt_reference):
            with self.assertRaises(xenos.FormatError):
                decode(8, 8, bytes(31), 18)

    def test_refpack_native_and_python_agree(self):
        # 0xE0 literal block of 4, then a stop command with 0 literals; header 0x10FB size 4.
        stream = bytes((0x10, 0xFB, 0, 0, 4, 0xE0, 1, 2, 3, 4, 0xFC))
        self.assertEqual(refpack.decompress(stream, 4), bytes((1, 2, 3, 4)))
        native, refpack._fast_decode = refpack._fast_decode, None
        try:
            self.assertEqual(refpack.decompress(stream, 4), bytes((1, 2, 3, 4)))
        finally:
            refpack._fast_decode = native


if __name__ == '__main__':
    unittest.main()
