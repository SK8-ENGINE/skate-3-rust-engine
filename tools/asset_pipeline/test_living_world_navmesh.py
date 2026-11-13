"""Tests for the NavPower navmesh decoder (synthetic objects only, no game data)."""
import struct
import unittest

from tools.asset_pipeline import living_world_navmesh as N


def _poly(centre, verts, neighbours, area=0x11):
    out = struct.pack('>4f', *centre, 1.0) + struct.pack('>II', N.FLT_MAX_BITS, 0)
    out += struct.pack('>III', 0x007FFF80, area << 8 | 0x00, (len(verts) - 2) << 24)
    for v, n in zip(verts, neighbours):
        out += struct.pack('>I3fII', n, *v, 0xFFFF0000, 0)
    return out + b'\0' * 12


def _object(x0, quads, base=0x80):
    """A NavPower-like object: header padding, agent block at ``base``, polygons of unit squares at
    x0 + i (each linked to the next)."""
    head = bytearray(b'\0' * base)
    head[0:4] = struct.pack('>I', 0x30)
    graph = struct.pack('>4f', 0.12, 0.35, 0.2, 1.6) + struct.pack('>6f', x0, 0, 0, x0 + quads - 0.14, 1, 1)
    first = base + len(graph)
    sizes = [36 + 4 * 24 + 12] * quads
    starts = [first + sum(sizes[:i]) for i in range(quads)]
    body = b''
    for i in range(quads):
        x = x0 + i
        verts = [(x, 0, 0), (x + 1, 0, 0), (x + 1, 0, 1), (x, 0, 1)]
        right = starts[i + 1] - base if i + 1 < quads else 0
        left = starts[i - 1] - base if i > 0 else 0
        body += _poly((x + .5, 0, .5), verts, [0, right, 0, left], 0xA1 if i == 1 else 0x11)
    return bytes(head) + graph + body + b'\0' * 64


class NavmeshTest(unittest.TestCase):
    def test_parse_layout(self):
        g = N.parse_graph(_object(0.0, 3))
        self.assertEqual([round(a, 2) for a in g['agent']], [0.12, 0.35, 0.2, 1.6])
        self.assertEqual(len(g['polygons']), 3)
        self.assertEqual([p['neighbours'] for p in g['polygons']], [[-1, 1, -1, -1], [-1, 2, -1, 0], [-1, -1, -1, 1]])
        self.assertEqual([p['area'] for p in g['polygons']], [0x11, 0xA1, 0x11])
        self.assertEqual(g['polygons'][2]['verts'][1], (3.0, 0.0, 0.0))

    def test_other_base_and_companion_objects(self):
        g = N.parse_graph(_object(0.0, 2, base=0xD0))
        self.assertEqual(len(g['polygons']), 2)
        self.assertIsNone(N.parse_graph(b'\0' * 88))

    def test_bad_neighbour_rejected(self):
        data = bytearray(_object(0.0, 2))
        first = 0x80 + 40  # agent block (16) + bounds (24)
        struct.pack_into('>I', data, first + 36 + 24, 0x1234)
        with self.assertRaises(ValueError):
            N.parse_graph(bytes(data))

    def test_tiles_stitch_across_the_gap_and_round_trip(self):
        a = N.parse_graph(_object(0.0, 2))
        b = N.parse_graph(_object(2.14, 2))
        mesh = N.merge_district([a, b])
        polys = mesh['polygons']
        self.assertEqual(polys[1]['neighbours'][1], 2, 'east edge of tile A joins tile B')
        self.assertEqual(polys[2]['neighbours'][3], 1)
        self.assertEqual(sum(p['stitched'] for p in polys), 2)
        data = N.write_navmesh({'DownTown': mesh})
        back = N.read_navmesh(data)['DownTown']
        self.assertEqual([p['neighbours'] for p in back['polygons']], [p['neighbours'] for p in polys])
        self.assertEqual(N.summary(back)['areas'], {'0x11': 2, '0xa1': 2})
        with self.assertRaises(ValueError):
            N.read_navmesh(b'NOPE' + data[4:])


if __name__ == '__main__':
    unittest.main()
