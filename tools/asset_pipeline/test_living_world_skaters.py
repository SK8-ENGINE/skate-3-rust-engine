"""NPC skater export helpers, checked on synthetic data. One optional test reads the
user's extracted disc when SKATE3_DISC_ROOT points at it (skipped otherwise)."""
import json
import os
import struct
import tempfile
import unittest
from pathlib import Path

from tools.asset_pipeline import living_world_skaters as lws
from tools.asset_pipeline.vlt import hash64


def path_id(tag: bytes, n: int) -> bytes:
    return b'\0' + tag + b'\0' + bytes(9) + bytes([n])


def aipath_blob(paths) -> bytes:
    """paths: [(id16, [(x, y, z, frames)], flags, mask, [(node, [(target id16, target node, weight)])])]"""
    out = bytearray(struct.pack('>II', len(paths), 0x10) + b'\xde' * 8 + bytes(0x60 * len(paths)))
    for i, (pid, nodes, flags, mask, groups) in enumerate(paths):
        h = 0x10 + 0x60 * i
        nodes_at = len(out)
        prev = None
        for x, y, z, frames in nodes:
            d = (0.0, 0.0, 0.0) if prev is None else (x - prev[0], y - prev[1], z - prev[2])
            out += struct.pack('>6f', x, y, z, *d) + bytes([128, 128, 128, 255, 128, 255, 128, 128])
            out += struct.pack('>I', 0) + bytes([frames, 255, 255, 0, 0, 0, 0, 0])
            prev = (x, y, z)
        groups_at = len(out)
        out += bytes(12 * len(groups))
        for g, (node, branches) in enumerate(groups):
            go = groups_at + 12 * g
            struct.pack_into('>III', out, go, len(out) - go, len(branches), node)
            for target, target_node, weight in branches:
                out += target + struct.pack('>If', target_node, weight)
        xs = [n[:3] for n in nodes]
        lo = [min(p[a] for p in xs) for a in range(3)]
        hi = [max(p[a] for p in xs) for a in range(3)]
        struct.pack_into('>4f4f', out, h, *lo, 1.0, *hi, 1.0)
        out[h + 0x20:h + 0x30] = pid
        struct.pack_into('>6IQi', out, h + 0x30, nodes_at - h, len(nodes), 0,
                         groups_at - h if groups else 0, len(groups), flags, mask, 0)
    return bytes(out)


def arena(objects) -> bytes:
    """A minimal RW4 arena: directory count at +0x20, offset at +0x30, 24-byte entries."""
    head = bytearray(0x40)
    directory = len(head)
    data_at = directory + 24 * len(objects)
    body = bytearray()
    entries = bytearray()
    for kind, blob in objects:
        entries += struct.pack('>6I', data_at + len(body), 0, len(blob), 0, 0, kind)
        body += blob
    struct.pack_into('>I', head, 0x20, len(objects))
    struct.pack_into('>I', head, 0x30, directory)
    return bytes(head + entries + body)


A, B = path_id(b'indu', 1), path_id(b'indu', 2)
TWO = aipath_blob([
    (A, [(0, 0, 0, 1), (3, 0, 4, 5)], 7, (1 << 62) - 1, [(1, [(B, 0, 0.5)])]),
    (B, [(3, 0, 4, 1), (3, 0, 10, 6)], 0x44, 1 << 51, []),
])
ONLY_B = aipath_blob([(B, [(3, 0, 4, 1), (3, 0, 10, 6)], 0x44, 1 << 51, [])])


class AiPathObjects(unittest.TestCase):
    def test_arena_objects_pick_the_aipath_type(self):
        objects = lws.arena_objects(arena([(0x00EB0010, b'tree'), (lws.AIPATHDATA, TWO), (0x00EB0011, b'k')]))
        self.assertEqual(objects, [TWO])

    def test_header_summary(self):
        a, b = lws.paths_in(TWO)
        self.assertEqual(a['id'], A.hex())
        self.assertEqual(a['tag'], b'indu')
        self.assertEqual((a['nodes'], a['flags'], a['allowed_skaters']), (2, 7, (1 << 62) - 1))
        self.assertEqual(a['start'], [0.0, 0.0, 0.0])
        self.assertAlmostEqual(a['length'], 5.0)
        self.assertEqual(a['branches'], [(1, B.hex(), 0)])
        self.assertEqual(a['bbox'], [[0.0, 0.0, 0.0], [3.0, 0.0, 4.0]])
        self.assertEqual(b['branches'], [])

    def test_truncated_blob_is_rejected(self):
        with self.assertRaises((ValueError, struct.error)):
            lws.paths_in(TWO[:0x50])
        with self.assertRaises((ValueError, struct.error)):
            lws.paths_in(TWO[:0x10 + 0x60 * 2 + 10])


class Pack(unittest.TestCase):
    def test_layout_matches_the_rust_reader(self):
        pack = lws.write_pack([(0x246DC82D654A0724, 'cSim_-150_-150_high', TWO), (7, 'cSim_-50_-150_high', ONLY_B)])
        self.assertEqual(pack[:8], b'LWSKPTH\0')
        self.assertEqual(struct.unpack_from('<II', pack, 8), (1, 2))
        asset_id, offset, length = struct.unpack_from('<QII', pack, 16)
        self.assertEqual((asset_id, length), (0x246DC82D654A0724, len(TWO)))
        self.assertEqual(offset % 16, 0)
        self.assertEqual(offset, (16 + 2 * 48 + 15) & ~15)
        self.assertEqual(pack[32:32 + 19], b'cSim_-150_-150_high')
        self.assertEqual(pack[offset:offset + length], TWO)
        self.assertEqual(lws.read_pack(pack), [(0x246DC82D654A0724, 'cSim_-150_-150_high', TWO),
                                               (7, 'cSim_-50_-150_high', ONLY_B)])

    def test_export_keeps_tiles_and_dedupes_by_id(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            index, warnings = lws.export_paths([('Industrial', 'cSim_-150_-150_high', 5, TWO),
                                                ('Industrial', 'cSim_-50_-150_high', 6, ONLY_B)], out)
            self.assertEqual(warnings, [])
            district = index['districts']['Industrial']
            self.assertEqual(district['path_copies'], 3)
            self.assertEqual(len(district['paths']), 2)
            self.assertEqual(district['paths'][B.hex()]['tiles'], ['cSim_-150_-150_high', 'cSim_-50_-150_high'])
            self.assertEqual(district['paths'][A.hex()]['allowed_skaters'], '3FFFFFFFFFFFFFFF')
            self.assertEqual(district['nodes'], 4)
            self.assertEqual(len(lws.read_pack((out/'Industrial.bin').read_bytes())), 2)
            self.assertEqual(json.loads((out/'index.json').read_text())['version'], 1)

    def test_unknown_branch_target_and_bad_tag_are_reported(self):
        lost = aipath_blob([(path_id(b'test', 1), [(0, 0, 0, 1)], 7, 1, [(0, [(path_id(b'indu', 9), 0, 1.0)])])])
        with tempfile.TemporaryDirectory() as tmp:
            _, warnings = lws.export_paths([('DownTown', 'cSim_50_50_high', 1, lost)], Path(tmp))
        self.assertEqual(len(warnings), 2)
        self.assertTrue(any('unknown path' in w for w in warnings))
        self.assertTrue(any("tag b'test'" in w for w in warnings))


def field(kind, data):
    return {'type': kind, 'data': data}


def refspec(cls, key):
    return f'{hash64(cls):016X}{hash64(key):016X}' + '0' * 16


COLLECTIONS = [
    {'class': 'ai_skater', 'key': 'default', 'parent': '', 'fields': {
        'Hash_281F55D7BB965ADC': field('EA::Reflection::Float', '41200000'),
        'Hash_D77533A1AAD7F117': field('EA::Reflection::Int32', '0000002D')}},
    {'class': 'ai_skater_profiles', 'key': 'default', 'parent': '', 'fields': {
        'ollie': field('EA::Reflection::Float', '3F800000'),
        lws.PROFILE_PRO_INDEX: field('Sk8::AIPath::EAISkater', '00000033')}},
    {'class': 'ai_skater_profiles', 'key': 'chris_cole', 'parent': 'default', 'fields': {
        lws.PROFILE_PRO_INDEX: field('Sk8::AIPath::EAISkater', '00000006'),
        'Hash_BB901D68361E9833': {'type': 'Sk8::AI::ProfileTrickEntry', 'data': '0002000200080000',
                                  'array': {'items': ['0000006A3F800000', '000000713F19999A']}}}},
    {'class': 'ai_skater_profiles', 'key': 'teammate_default', 'parent': 'default', 'fields': {}},
    {'class': 'characters_marquee', 'key': 'default', 'parent': '', 'fields': {
        lws.MARQUEE_FIELDS['recipe']: field('EA::Reflection::Text', ''),
        lws.MARQUEE_FIELDS['layout_8']: field('EA::Reflection::Bool', '00'),
        lws.MARQUEE_FIELDS['layout_9']: field('EA::Reflection::Bool', '00'),
        lws.MARQUEE_FIELDS['voice']: field('Sk8::Audio::eSk8Characters', '00000000'),
        'aiprofile': field('Attrib::RefSpec', refspec('ai_skater_profiles', 'default'))}},
    {'class': 'characters_marquee', 'key': 'pro_skaters', 'parent': 'default', 'fields': {
        lws.MARQUEE_FIELDS['layout_9']: field('EA::Reflection::Bool', '01')}},
    {'class': 'characters_marquee', 'key': 'chris_cole', 'parent': 'pro_skaters', 'fields': {
        lws.MARQUEE_FIELDS['recipe']: field('EA::Reflection::Text', 'chris_cole'),
        lws.MARQUEE_FIELDS['layout_8']: field('EA::Reflection::Bool', '01'),
        lws.MARQUEE_FIELDS['voice']: field('Sk8::Audio::eSk8Characters', '00000002'),
        lws.MARQUEE_FIELDS['aiprofile']: field('Attrib::RefSpec', refspec('ai_skater_profiles', 'chris_cole'))}},
    {'class': 'characters_marquee', 'key': 'teammates', 'parent': 'default', 'fields': {
        lws.MARQUEE_FIELDS['layout_9']: field('EA::Reflection::Bool', '01'),
        lws.MARQUEE_FIELDS['teammate']: field('EA::Reflection::Bool', '01000000'),
        lws.MARQUEE_FIELDS['aiprofile']: field('Attrib::RefSpec', refspec('ai_skater_profiles', 'teammate_default'))}},
    {'class': 'characters_marquee', 'key': 'teammate_02', 'parent': 'teammates', 'fields': {
        lws.MARQUEE_FIELDS['recipe']: field('EA::Reflection::Text', 'teammate_02'),
        lws.MARQUEE_FIELDS['teammate_index']: field('EA::Reflection::UInt8', '02000000')}},
    {'class': 'characters_marquee', 'key': 'ambient_skater_01', 'parent': 'default', 'fields': {
        lws.MARQUEE_FIELDS['recipe']: field('EA::Reflection::Text', 'ai_skater_01'),
        lws.MARQUEE_FIELDS['layout_8']: field('EA::Reflection::Bool', '01')}},
]


class Profiles(unittest.TestCase):
    def setUp(self):
        self.doc, self.warnings = lws.profiles(COLLECTIONS)

    def test_no_warnings_and_tables_present(self):
        self.assertEqual(self.warnings, [])
        self.assertEqual(self.doc['ai_skater']['default']['fields']['Hash_281F55D7BB965ADC'], 10.0)
        self.assertEqual(self.doc['ai_skater']['default']['fields']['Hash_D77533A1AAD7F117'], 45)

    def test_profiles_inherit_and_decode_trick_tables(self):
        cole = self.doc['ai_skater_profiles']['chris_cole']['fields']
        self.assertEqual(cole['ollie'], 1.0)
        self.assertEqual(cole[lws.PROFILE_PRO_INDEX], 6)
        tricks = cole['Hash_BB901D68361E9833']
        self.assertEqual(tricks[0], {'trick': 0x6A, 'weight': 1.0})
        self.assertAlmostEqual(tricks[1]['weight'], 0.6, places=6)

    def test_characters_resolve_profile_layout_bytes_and_pool(self):
        chars = self.doc['characters']
        cole = chars['chris_cole']
        self.assertEqual((cole['recipe'], cole['aiprofile'], cole['pro_index'], cole['voice']), ('chris_cole', 'chris_cole', 6, 2))
        self.assertEqual(cole['layout']['+8'], True)
        self.assertEqual(cole['layout']['+9'], True)
        mate = chars['teammate_02']
        self.assertEqual((mate['teammate'], mate['teammate_index'], mate['aiprofile'], mate['pro_index']),
                         (True, 2, 'teammate_default', 51))
        self.assertEqual(mate['needs'], 'recruited_save_slot')
        # Plain-named and hashed field forms both resolve (the inherited 'aiprofile').
        self.assertEqual(chars['ambient_skater_01']['aiprofile'], 'default')
        self.assertFalse(chars['ambient_skater_01']['free_roam_pool'])
        self.assertFalse(chars['pro_skaters']['free_roam_pool'])  # group record, no recipe
        self.assertEqual(self.doc['free_roam_pool'], ['chris_cole', 'teammate_02'])

    def test_missing_profile_is_reported(self):
        rows = COLLECTIONS + [{'class': 'characters_marquee', 'key': 'ghost', 'parent': 'default', 'fields': {
            lws.MARQUEE_FIELDS['aiprofile']: field('Attrib::RefSpec', refspec('ai_skater_profiles', 'nobody'))}}]
        _, warnings = lws.profiles(rows)
        self.assertEqual(len(warnings), 1)


class NpcCharacterPool(unittest.TestCase):
    """native_roster.npc_pool / bind_teammates: the free-roam pool and the teammate hook."""

    def setUp(self):
        from tools.asset_pipeline import native_roster
        self.roster = native_roster
        community = {'class': 'characters_marquee', 'key': 'community_skater_01', 'parent': 'default', 'fields': {
            lws.MARQUEE_FIELDS['recipe']: field('EA::Reflection::Text', 'community_skater_01'),
            lws.MARQUEE_FIELDS['layout_9']: field('EA::Reflection::Bool', '01'),
            lws.MARQUEE_FIELDS['community']: field('EA::Reflection::Bool', '01000000')}}
        self.rows = COLLECTIONS + [community]

    def test_pool_is_byte_9_records_with_their_look_source(self):
        pool = {item['key']: item for item in self.roster.npc_pool(self.rows)}
        self.assertEqual(sorted(pool), ['chris_cole', 'community_skater_01', 'teammate_02'])
        self.assertEqual(pool['chris_cole']['source'], 'marquee')
        self.assertEqual(pool['chris_cole']['recipe'], 'chris_cole')
        self.assertEqual(pool['teammate_02'], {'key': 'teammate_02', 'source': 'save', 'teammate_index': 2})
        self.assertEqual(pool['community_skater_01']['source'], 'online')

    def test_teammate_binding_needs_an_existing_library_entry(self):
        pool = self.roster.npc_pool(self.rows)
        with tempfile.TemporaryDirectory() as tmp:
            entry = Path(tmp)/'entries'/'abc123'
            entry.mkdir(parents=True)
            (entry/'character.glb').write_bytes(b'glTF')
            ready, problems = self.roster.bind_teammates(pool, tmp, {'teammate_02': 'abc123'})
            self.assertEqual((ready, problems), ({'teammate_02': entry}, []))
            ready, problems = self.roster.bind_teammates(pool, tmp, {'teammate_02': None, 'chris_cole': 'abc123'})
            self.assertEqual(ready, {})
            self.assertEqual(problems, ['chris_cole is not a teammate record'])
            ready, problems = self.roster.bind_teammates(pool, tmp, {'teammate_02': '../x'})
            self.assertEqual(ready, {})
            self.assertEqual(len(problems), 1)


@unittest.skipUnless(os.environ.get('SKATE3_DISC_ROOT'), 'set SKATE3_DISC_ROOT to the extracted disc to run')
class Disc(unittest.TestCase):
    def test_export_totals(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = lws.export({'game_root': Path(os.environ['SKATE3_DISC_ROOT']),
                                 'private': Path(tmp)/'private', 'work': Path(tmp)/'work'})
        self.assertEqual(report['warnings'], [])
        self.assertEqual(report['path_copies'], 3891)
        self.assertEqual(report['paths'], 1691)
        self.assertEqual(sorted(report['districts']), ['DownTown', 'Industrial', 'University'])
        self.assertEqual(report['characters'], 87)
        self.assertEqual(report['profiles'], 193)
        self.assertEqual(report['free_roam_pool'], 42)


if __name__ == '__main__':
    unittest.main()
