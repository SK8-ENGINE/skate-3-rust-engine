import struct
import unittest

from .grab_data import GRABDATA, grab_splines
from .test_dynamic_props import resource


def grabdata(points, enabled=1):
    """One GRABDATA section with one entry (the stock car layout: entries at +32, points at +112)."""
    n = len(points)
    size = 112 + n * 16 + 16
    raw = bytearray(size)
    struct.pack_into('>7I', raw, 0, 1, n, 0, 1 if enabled else 0, 32, 112, 112 + n * 16)
    struct.pack_into('>3f', raw, 32, -0.8, 0.9, -1.9)
    struct.pack_into('>3f', raw, 48, 0.8, 0.9, -1.7)
    struct.pack_into('>I', raw, 32 + 48, 112)
    struct.pack_into('>I', raw, 32 + 56, 112 + n * 16)
    struct.pack_into('>I', raw, 32 + 60, 0x3E4)
    raw[32 + 68] = n
    raw[32 + 70] = enabled
    for k, p in enumerate(points):
        struct.pack_into('>4f', raw, 112 + k * 16, *p, 1.0)
    struct.pack_into('>4f', raw, 112 + n * 16, 0.0, 0.0, -1.0, 0.0)
    return bytes(raw)


class GrabDataTests(unittest.TestCase):
    def test_reads_the_rear_edge_spline_in_the_model_frame(self):
        points = [(-0.8, 0.9, -1.7), (-0.8, 0.9, -1.7), (0.8, 0.9, -1.7), (0.8, 0.9, -1.7)]
        result = grab_splines(resource(GRABDATA, grabdata(points)))
        self.assertEqual(len(result), 1)
        s = result[0]
        self.assertEqual(len(s['control_points']), 4)
        self.assertAlmostEqual(s['control_points'][2][0], 0.8, places=5)
        self.assertEqual(s['direction'], [0.0, 0.0, -1.0])
        self.assertEqual(s['flags'], 0x3E4)

    def test_disabled_entries_are_skipped_and_bad_layouts_raise(self):
        points = [(0.0, 0.0, 0.0)] * 4
        self.assertEqual(grab_splines(resource(GRABDATA, grabdata(points, enabled=0))), [])
        with self.assertRaises(ValueError):
            grab_splines(resource(GRABDATA, grabdata(points[:3])))
        self.assertEqual(grab_splines(resource(0xEB001D, grabdata(points))), [])
        wrong = bytearray(grabdata(points, enabled=0))
        struct.pack_into('>I', wrong, 12, 1)
        with self.assertRaises(ValueError):
            grab_splines(resource(GRABDATA, bytes(wrong)))
