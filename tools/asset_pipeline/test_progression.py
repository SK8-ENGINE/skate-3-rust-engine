import struct
import unittest

from tools.asset_pipeline.progression import (ACTION_CLASS, ACTION_GRAPH, CHALLENGE_CLASS, GRAPH_CLASS, GROUP_CLASS,
                                              KILLED_REWARD, OWNED_REWARD, RECORD_FIRED, REWARD_CLASS, REWARD_KIND,
                                              REWARD_NAME, TRIGGERS, challenge_rewards, progression)
from tools.asset_pipeline.vlt import hash64


def row(cls, key, fields, parent=''):
    return {'class': cls, 'key': key, 'parent': parent, 'fields': fields}


def trigger(kind, ref, threshold):
    return struct.pack('>IIQQ', kind, 0, ref, threshold).hex().upper()


def typed(key):
    return struct.pack('>QQ', key, 0).hex().upper()


def refspec(cls, key):
    return struct.pack('>QQQ', int(cls[5:], 16), key, 0).hex().upper()


class Progression(unittest.TestCase):
    def setUp(self):
        self.rows = [
            row(CHALLENGE_CLASS, 'ots_a', {}),
            row(CHALLENGE_CLASS, 'ots_b', {}),
            row(GRAPH_CLASS, 'milestone_1', {}),
            row(GROUP_CLASS, 'ots_all', {'Hash_0000000000000001': {'type': 'Attrib::Gen::ClassRefSpec_challenges',
                'array': {'items': [typed(hash64('ots_a')), typed(hash64('ots_b'))]}}}),
            row(ACTION_CLASS, 'milestone_01', {
                TRIGGERS: {'type': 'tObservatoryTrigger', 'array': {'items': [
                    trigger(0, 0, 15000), trigger(1, hash64('ots_all'), 2), trigger(2, hash64('ots_a'), 0)]}},
                ACTION_GRAPH: {'type': 'Attrib::Gen::ClassRefSpec_progression_stategraph',
                               'data': typed(hash64('milestone_1'))}}),
            row(ACTION_CLASS, 'starting_action', {RECORD_FIRED: {'type': 'Bool', 'data': '01'}}),
            row(ACTION_CLASS, 'milestone_02', {}, parent='milestone_actions'),
            row(REWARD_CLASS, 'locations', {REWARD_KIND: {'type': 'Enum', 'data': '00000000'}}),
            row(REWARD_CLASS, 'pcu_training', {REWARD_NAME: {'type': 'EA::Reflection::Text', 'data': 'pcu_training'}},
                parent='locations'),
        ]

    def test_actions_decode_their_and_list_and_graph(self):
        p = progression(self.rows)
        a = p['actions']['milestone_01']
        self.assertEqual(a['conditions'], [
            {'type': 'board_sales', 'threshold': 15000},
            {'type': 'group_done', 'threshold': 2, 'ref': 'ots_all'},
            {'type': 'challenge_done', 'threshold': 0, 'ref': 'ots_a'}])
        self.assertEqual(a['graph'], 'milestone_1')
        self.assertFalse(a['record_fired'])
        self.assertEqual(p['actions']['starting_action'],
                         {'parent': None, 'conditions': [], 'graph': None, 'record_fired': True})
        self.assertEqual(p['actions']['milestone_02']['parent'], 'milestone_actions')

    def test_killable_from_a_killed_it_list_or_the_race_type(self):
        from tools.asset_pipeline.challenges import CHALLENGE_TYPE, KILLED
        from tools.asset_pipeline.progression import killable
        self.assertFalse(killable({}))
        self.assertFalse(killable({KILLED: {'array': {'items': []}}, CHALLENGE_TYPE: {'data': '00000009'}}))
        self.assertTrue(killable({KILLED: {'array': {'items': [refspec(GROUP_CLASS, 1)]}}}))
        self.assertTrue(killable({CHALLENGE_TYPE: {'data': '00000013'}}))

    def test_groups_list_their_challenges_and_rewards_inherit_the_kind(self):
        p = progression(self.rows)
        self.assertEqual(p['groups']['ots_all'], ['ots_a', 'ots_b'])
        self.assertEqual(p['rewards']['pcu_training']['kind'], 0)
        self.assertEqual(p['rewards']['pcu_training']['name'], 'pcu_training')

    def test_references_into_other_banks_resolve_through_known_names(self):
        rows = [r for r in self.rows if r['class'] != CHALLENGE_CLASS]
        self.assertEqual(progression(rows)['groups']['ots_all'], ['Hash_%016X' % hash64('ots_a'), 'Hash_%016X' % hash64('ots_b')])
        known = {hash64('ots_a'): 'ots_a', hash64('ots_b'): 'ots_b'}
        self.assertEqual(progression(rows, known)['groups']['ots_all'], ['ots_a', 'ots_b'])

    def test_presentation_events_decode_type_names_and_string_args(self):
        from tools.asset_pipeline.progression import presentation_events
        binary = bytes(1) + b'loc_a' + bytes(1) + b'dist_x' + bytes(1)
        items = [struct.pack('>IIII', 8, 1, 7, 0).hex(), struct.pack('>IIII', 68, 0, 0, 0).hex()]
        self.assertEqual(presentation_events({'array': {'items': items}}, binary), [
            {'type': 'TeleportLocalPlayer', 'args': ['loc_a', 'dist_x'], 'mask': 0},
            {'type': 'ShowMapTutorial', 'args': [None, None], 'mask': 0}])
        with self.assertRaises(ValueError):
            presentation_events({'array': {'items': [struct.pack('>IIII', 99, 0, 0, 0).hex()]}}, binary)
        with self.assertRaises(ValueError):
            presentation_events({'array': {'items': [struct.pack('>IIII', 5, 2, 0, 0).hex()]}}, binary)

    def test_challenge_rewards_read_money_sales_and_reward_ids(self):
        binary = bytes(16) + bytes.fromhex(refspec(REWARD_CLASS, hash64('pcu_training')))
        fields = {OWNED_REWARD: {'type': 'tObservatoryProgressionReward', 'data': struct.pack('>IIII', 0, 6000, 16, 1).hex()},
                  KILLED_REWARD: {'type': 'tObservatoryProgressionReward', 'data': struct.pack('>IIII', 0, 1500, 0, 0).hex()}}
        names = {hash64('pcu_training'): 'pcu_training'}
        self.assertEqual(challenge_rewards(fields, binary, names), {
            'owned': {'money': 0, 'sales': 6000, 'rewards': ['pcu_training']},
            'killed': {'money': 0, 'sales': 1500, 'rewards': []}})


if __name__ == '__main__':
    unittest.main()
