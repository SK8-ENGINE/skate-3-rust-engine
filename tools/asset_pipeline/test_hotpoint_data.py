import struct
import unittest

from .hotpoint_data import HOTPOINTDATA, RECORD_SIZE, section_hotpoints


def section(points):
    out = bytearray(struct.pack('>4I', len(points), 16, 0, 0))
    for kind, position, forward in points:
        m = [1, 0, 0, 0, 0, 1, 0, 0, *forward, 0, *position, 1]
        record = bytearray(RECORD_SIZE)
        struct.pack_into('>16f', record, 0, *m)
        struct.pack_into('>I', record, 96, kind)
        record[100] = 1
        out += record
    return bytes(out)


class HotpointDataTest(unittest.TestCase):
    def test_records_parse_in_the_model_frame(self):
        raw = section([(6, (0.5, 0.0, 0.2), (0.0, 0.0, 1.0)), (1, (0.0, 0.8, 0.0), (0.0, 0.0, -1.0))])
        found = section_hotpoints(raw, 0, len(raw))
        self.assertEqual([h['type'] for h in found], [6, 1])
        self.assertEqual(found[0]['position'], [0.5, 0.0, 0.20000000298023224])
        self.assertEqual(found[1]['axes'][2], [0.0, 0.0, -1.0])
        self.assertEqual(found[0]['extra'], 1)

    def test_a_wrong_header_or_matrix_raises(self):
        raw = bytearray(section([(6, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0))]))
        with self.assertRaises(ValueError):
            section_hotpoints(bytes(raw), 0, len(raw) - 4)
        struct.pack_into('>f', raw, 16 + 60, 2.0)
        with self.assertRaises(ValueError):
            section_hotpoints(bytes(raw), 0, len(raw))
        self.assertEqual(HOTPOINTDATA, 0x00EB001E)


class HotpointPropsTest(unittest.TestCase):
    def test_seats_follow_the_placement_and_types_map_to_classes(self):
        import numpy as np
        from .dynamic_props import hotpoint_props
        from .hotpoint_data import CLASSES
        # A bench turned 90 degrees about y (row vectors: x -> -z, z -> x) and moved to (10, 1, 20).
        transform = np.array([[0, 0, -1, 0], [0, 1, 0, 0], [1, 0, 0, 0], [10, 1, 20, 1]], dtype=float)
        hotpoints = [dict(type=0, position=[0, 0, 0], axes=[[1, 0, 0], [0, 1, 0], [0, 0, 1]]),
                     dict(type=6, position=[0.5, 0, 0.25], axes=[[1, 0, 0], [0, 1, 0], [0, 0, 1]]),
                     dict(type=8, position=[0, 0, 0], axes=[[1, 0, 0], [0, 1, 0], [0, 0, 1]]),
                     dict(type=1, position=[0, 0.8, 0], axes=[[1, 0, 0], [0, 1, 0], [0, 0, -1]])]
        props = hotpoint_props('BBDDDCD5E794B3CE', 'T', transform, hotpoints, CLASSES)
        self.assertEqual([(p['index'], p['class']) for p in props], [(1, 'waypoint_sit'), (3, 'waypoint_usetrashbin')])
        self.assertEqual(props[0]['position'], [10.25, 1.0, 19.5])
        self.assertEqual(props[0]['facing'], [1.0, 0.0, 0.0])
        self.assertEqual(props[1]['facing'], [-1.0, 0.0, 0.0])


if __name__ == '__main__':
    unittest.main()
