import json
import struct
import tempfile
import unittest
from pathlib import Path

from tools.asset_pipeline.challenges import (CHALLENGE, DEFINITION, ENTERED, GROUP, KILLED_TIME, OBJECTIVES,
                                             OUT_OF_BOUNDS_TIME, OWNED_TIME, POINTS, RETURN_MESSAGE, chain,
                                             group_points, objective_programs, parameters)
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

    def test_objective_conditions_export_as_programs(self):
        text = b'do return Lib.Done() end'
        binary = bytes(1) + text + bytes(1)
        objective = {'key': 'obj_a_0', 'fields': {DEFINITION: {'data': struct.pack('>II', 1, 0).hex().upper()}}}
        group = {'key': 'obj_a', 'fields': {OBJECTIVES: {'array': {'items': [
            struct.pack('>QQQ', int(GROUP[5:], 16), hash64('obj_a_0'), 0).hex().upper()]}}}}
        rows = {(GROUP, hash64('obj_a')): group, (GROUP, hash64('obj_a_0')): objective}
        out = objective_programs(rows, refspec(GROUP, hash64('obj_a')), binary)
        self.assertEqual(out, [{'key': 'obj_a_0', 'condition': {'locals': 0, 'body': [
            {'return': {'api': 'Lib.Done', 'args': []}}]}, 'title_id': None, 'title_arg': None, 'display_in_hud': True,
            'triggers': [False] * 14, 'activation': 0,
            'failures': [], 'failures_inline': [], 'failure_first': False, 'hints': []}])
        self.assertEqual(objective_programs(rows, None, binary), [])
        # the HUD title: tObjectiveDefinition word 3 is the offset of its text id in the bank .bin (b947)
        titled = binary + b'ID_OBJ_A_0' + bytes(1)
        objective['fields'][DEFINITION]['data'] = struct.pack('>IIIII', 1, 0, 0, len(binary), 0).hex().upper()
        self.assertEqual(objective_programs(rows, refspec(GROUP, hash64('obj_a')), titled)[0]['title_id'], 'ID_OBJ_A_0')

    def test_points_valid_is_inherited_and_read_from_the_parents_bank(self):
        from tools.asset_pipeline.challenges import POINTS_VALID
        own = bytes(1) + b'return true' + bytes(1)
        main = bytes(3) + b'return Lib.Newly()' + bytes(1)
        from tools.asset_pipeline.challenges import ACTIVATION_MODE, TRIGGERS
        family = {'key': 'family', 'class': GROUP, 'parent': '', 'fields': {
            POINTS_VALID: {'data': struct.pack('>I', 3).hex()}, ACTIVATION_MODE: {'data': struct.pack('>I', 5).hex()},
            TRIGGERS: {'data': (bytes([0, 1]) + bytes(18)).hex()}}}
        objective = {'key': 'obj_a_0', 'class': GROUP, 'parent': 'family',
                     'fields': {DEFINITION: {'data': struct.pack('>II', 1, 0).hex()}}}
        group = {'key': 'obj_a', 'class': GROUP, 'parent': '', 'fields': {OBJECTIVES: {'array': {'items': [
            struct.pack('>QQQ', int(GROUP[5:], 16), hash64('obj_a_0'), 0).hex().upper()]}}}}
        rows = {(GROUP, hash64('obj_a')): group, (GROUP, hash64('obj_a_0')): objective, (GROUP, hash64('family')): family}
        out = objective_programs(rows, refspec(GROUP, hash64('obj_a')), own, lambda row: main if row['key'] == 'family' else own)
        self.assertEqual(out[0]['points_valid'], {'locals': 0, 'body': [{'return': {'api': 'Lib.Newly', 'args': []}}]})
        self.assertEqual((out[0]['activation'], out[0]['triggers'][:3]), (5, [False, True, False]))
        self.assertNotIn('points_valid', objective_programs({(GROUP, hash64('obj_a')): group,
            (GROUP, hash64('obj_a_0')): dict(objective, parent='')}, refspec(GROUP, hash64('obj_a')), own)[0])

    def test_spot_condition_names_its_volume(self):
        text = ('do\n\treturn ObjectiveTrigger.EnteredVolume("2446202983646982", '
                '"DIST_DownTown|ots_a_spotvolume_01" );\nend;')
        match = ENTERED.search(text)
        self.assertEqual((match.group(1), match.group(2), match.group(3)),
                         ('2446202983646982', 'DIST_DownTown', 'ots_a_spotvolume_01'))
        self.assertEqual('%016x' % int(match.group(1)), '0008b0cf03e38706')

    def test_parameters_inherit_from_the_framework_family_then_default(self):
        text = lambda s: {'type': 'EA::Reflection::Text', 'data': s}
        floats = lambda v: {'type': 'EA::Reflection::Float', 'data': struct.pack('>f', v).hex().upper()}
        framework = {
            hash64('default'): ('', {RETURN_MESSAGE: text('ID_DEFAULT'), OWNED_TIME: floats(3.1), KILLED_TIME: floats(3.3),
                                     OUT_OF_BOUNDS_TIME: points(5)}),
            0xBF65AB4C87F0A118: ('default', {RETURN_MESSAGE: text('ID_OTS')}),
        }
        row = {'parent': 'Hash_BF65AB4C87F0A118', 'fields': {KILLED_TIME: floats(4.0)}}
        p = parameters(row, framework)
        self.assertEqual((p['out_of_bounds_time'], p['return_to_area_message']), (5, 'ID_OTS'))
        self.assertAlmostEqual(p['owned_display_time'], 3.1, places=5)
        self.assertAlmostEqual(p['killed_display_time'], 4.0, places=5)
        self.assertEqual(parameters({'parent': 'missing', 'fields': {}}, framework)['out_of_bounds_time'], None)

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
