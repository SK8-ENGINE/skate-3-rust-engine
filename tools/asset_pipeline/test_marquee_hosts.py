import struct
import unittest
from .marquee_hosts import CHALLENGE_CLASS, HOSTS_FIELD, hosts
from .vlt import hash64


def element(entity, pointer, flag):
    raw = bytearray(40)
    struct.pack_into('>2Q', raw, 0, 0x60D703C77CAB1631, entity)
    struct.pack_into('>2I', raw, 24, pointer, flag)
    return raw.hex().upper()


def record(key, *items):
    return {'class': CHALLENGE_CLASS, 'key': key,
            'fields': {HOSTS_FIELD: {'array': {'items': list(items)}}}}


class MarqueeHostTests(unittest.TestCase):
    def test_hosts_resolve_by_exact_locator_name(self):
        binary = b'\0spot_a\0Spot_B\0'
        matrix = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [5, 6, 7, 1]]
        locators = {'spot_a': [{'map': 'DownTown', 'matrix': matrix}],
                    'spot_b': [{'map': 'DownTown', 'matrix': matrix}]}
        rows = hosts([record('b_02', element(7, 8, 2)), record('a_01', element(9, 1, 0), element(9, 0, 0)),
                      {'class': 'other', 'key': 'x', 'fields': {}}], binary, locators)
        self.assertEqual([(r['challenge'], r['element'], r['locator']) for r in rows],
                         [('a_01', 0, 'spot_a'), ('b_02', 0, 'Spot_B')])
        self.assertEqual(rows[0]['entity'], '0000000000000009')
        named = hosts([record('a', element(hash64('seb'), 1, 0))], binary, locators, ['seb', 'cuz'])
        self.assertEqual(named[0]['entity_name'], 'seb')
        self.assertIsNone(rows[0]['entity_name'])
        self.assertEqual((rows[0]['map'], rows[0]['matrix']), ('DownTown', matrix))
        # Retail hashes the raw bytes: a name that differs in case stays unplaced (pending).
        self.assertEqual((rows[1]['flag'], rows[1]['matrix']), (2, None))

    def test_a_name_with_two_different_spots_is_rejected(self):
        a = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]
        b = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [1, 0, 0, 1]]
        locators = {'spot': [{'map': 'A', 'matrix': a}, {'map': 'B', 'matrix': b}]}
        with self.assertRaisesRegex(ValueError, 'Ambiguous'):
            hosts([record('c', element(1, 1, 0))], b'\0spot\0', locators)


if __name__ == '__main__':
    unittest.main()
