import unittest

from tools.asset_pipeline.living_world_anim import resolve_anim_names, string_at


def pool(*names):
    blob = b'\0'
    offsets = {}
    for n in names:
        offsets[n] = len(blob)
        blob += n.encode() + b'\0'
    return blob, offsets


class AnimNames(unittest.TestCase):
    def test_offsets_resolve_to_clip_names(self):
        blob, at = pool('NPC_WNDR_WLK_N_0_CYC', 'NPC_WNDR_STND_IDLE1_0_CYC')
        self.assertEqual(string_at(blob, at['NPC_WNDR_WLK_N_0_CYC']), 'NPC_WNDR_WLK_N_0_CYC')
        self.assertIsNone(string_at(blob, at['NPC_WNDR_WLK_N_0_CYC'] + 2))  # mid-string
        self.assertIsNone(string_at(blob, 0))
        self.assertIsNone(string_at(blob, len(blob) + 5))
        self.assertIsNone(string_at(blob, True))

    def test_speed_rows_gain_named_states_tiers_and_turn_lists(self):
        import struct
        blob, at = pool('LongPadding', 'Walk', 'Brisk', 'Idle', 'ChaseRun')
        word = lambda f: struct.unpack('<I', struct.pack('<f', f))[0]
        row = lambda th, *w: {f'u32_{i * 4}': v for i, v in enumerate([word(th), *w])}
        rows = [row(2.0, at['Walk'], 8, at['Idle'], at['Walk'], 8, 8, 8),
                row(4.0, at['Walk'], at['Brisk'], at['Idle'], at['ChaseRun'], 8, 8, 8)]
        doc = {'classes': {'livingworld_entity_animation': {'default': {'parent': None, 'fields': {
            'Hash_9C9A4F2CB1CC45DE': rows}}}}}
        self.assertEqual(resolve_anim_names(doc, blob), 1)
        f = doc['classes']['livingworld_entity_animation']['default']['fields']
        self.assertEqual(f['speed_rows'], [
            {'threshold': 2.0, 'state': 'Walk', 'tier': None, 'turn_from': ['Idle', 'Walk']},
            {'threshold': 4.0, 'state': 'Walk', 'tier': 'Brisk', 'turn_from': ['Idle', 'ChaseRun']}])
        self.assertIs(f['Hash_9C9A4F2CB1CC45DE'], rows)

    def test_only_animation_structs_gain_names_and_numbers_stay(self):
        blob, at = pool('A_CLIP', 'B_CLIP', 'C_CLIP')
        doc = {'classes': {
            'livingworld_entity_animation': {'default': {'parent': None, 'fields': {
                'Hash_1': {'anim': at['A_CLIP'], 'u32_4': 3},
                'Hash_2': [{'anim': at['B_CLIP']}, {'anim': 7}],
                'Hash_3': 'PLAIN_TEXT'}}},
            'livingworld_entity_takedown': {'default': {'parent': None, 'fields': {
                'takedowns': [{'anim': at['A_CLIP'], 'anim_b': at['C_CLIP']}]}}},
            'livingworld_models': {'x': {'parent': None, 'fields': {'v': {'anim': at['A_CLIP']}}}}}}
        self.assertEqual(resolve_anim_names(doc, blob), 4)
        f = doc['classes']['livingworld_entity_animation']['default']['fields']
        self.assertEqual(f['Hash_1'], {'anim': at['A_CLIP'], 'u32_4': 3, 'anim_name': 'A_CLIP'})
        self.assertEqual(f['Hash_2'][0]['anim_name'], 'B_CLIP')
        self.assertNotIn('anim_name', f['Hash_2'][1])
        t = doc['classes']['livingworld_entity_takedown']['default']['fields']['takedowns'][0]
        self.assertEqual((t['anim_name'], t['anim_b_name']), ('A_CLIP', 'C_CLIP'))
        self.assertNotIn('anim_name', doc['classes']['livingworld_models']['x']['fields']['v'])
        self.assertEqual(resolve_anim_names(doc, None), 0)


if __name__ == '__main__':
    unittest.main()
