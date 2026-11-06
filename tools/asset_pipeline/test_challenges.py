import json
import struct
import tempfile
import unittest
from pathlib import Path

from tools.asset_pipeline.challenges import CHALLENGE, GROUP, POINTS, chain, group_points
from tools.asset_pipeline.retained import carry
from tools.asset_pipeline.vlt import hash64


def refspec(cls, key):
    raw = struct.pack('>QQQ', int(cls[5:], 16), key, 0)
    return {'type': 'Attrib::RefSpec', 'array': {'items': [raw.hex().upper()]}}


def points(value):
    return {'type': 'EA::Reflection::Int32', 'data': struct.pack('>i', value).hex().upper()}


class ChallengeData(unittest.TestCase):
    def test_chain_takes_own_fields_first_then_the_family(self):
        rows = {(CHALLENGE, 'ots_a'): ('own_the_spots', {'x': 1}),
                (CHALLENGE, 'own_the_spots'): ('main', {'x': 2, 'y': 3}),
                (CHALLENGE, 'main'): ('', {'z': 4})}
        fields, walked = chain(rows, CHALLENGE, 'OTS_A')
        self.assertEqual(fields, {'x': 1, 'y': 3, 'z': 4})
        self.assertEqual(walked, ['ots_a', 'own_the_spots', 'main'])

    def test_chain_stops_on_a_parent_loop(self):
        rows = {(CHALLENGE, 'a'): ('b', {}), (CHALLENGE, 'b'): ('a', {})}
        self.assertEqual(chain(rows, CHALLENGE, 'a')[1], ['a', 'b'])

    def test_group_points_by_named_and_hashed_keys(self):
        named = {'key': 'ots_a_0', 'fields': {POINTS: points(1000)}}
        hashed = {'key': 'Hash_0D9EECD4FCD859B4', 'fields': {POINTS: points(1500)}}
        rows = {(GROUP, hash64('ots_a_0')): named, (GROUP, 0x0D9EECD4FCD859B4): hashed}
        self.assertEqual(group_points(rows, refspec(GROUP, hash64('ots_a_0'))), 1000)
        self.assertEqual(group_points(rows, refspec(GROUP, 0x0D9EECD4FCD859B4)), 1500)
        self.assertIsNone(group_points(rows, refspec(GROUP, 1)))
        self.assertIsNone(group_points(rows, None))

    def test_kept_dlc_map_keeps_its_challenges(self):
        with tempfile.TemporaryDirectory() as folder:
            old, new = Path(folder) / 'old', Path(folder) / 'new'
            old.mkdir()
            new.mkdir()
            rows = [{'id': 'ots_dwtn_01', 'map': 'DownTown'}, {'id': 'ots_dlc_01', 'map': 'DlcMap'}]
            (old / 'challenges.json').write_text(json.dumps({'format': 'skate3rust.challenges', 'version': 1,
                                                             'challenges': rows}), encoding='utf-8')
            (new / 'challenges.json').write_text(json.dumps({'format': 'skate3rust.challenges', 'version': 1,
                                                             'challenges': rows[:1]}), encoding='utf-8')
            counts = carry(old, new, {'DlcMap'})
            self.assertEqual(counts['challenges'], 1)
            ids = [c['id'] for c in json.loads((new / 'challenges.json').read_text(encoding='utf-8'))['challenges']]
            self.assertEqual(ids, ['ots_dwtn_01', 'ots_dlc_01'])


if __name__ == '__main__':
    unittest.main()
