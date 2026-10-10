"""Living world (ped half) setup helpers, checked on synthetic data (never game data)."""
import json
import struct
import tempfile
import unittest
from pathlib import Path

from tools.asset_pipeline import living_world as lw
from tools.asset_pipeline.vlt import hash64


def ref(cls, key):
    return struct.pack('>QQ', hash64(cls), hash64(key) if key else 0)


def f32(value):
    return struct.pack('>f', value).hex().upper()


def row(cls, key, parent='', **fields):
    return {'class': cls, 'key': key, 'parent': parent, 'fields': fields}


def collections():
    """A tiny census chain: census -> group -> category -> entities -> models (one a group record)."""
    census_entry = (ref('livingworld_categorygroups', 'downtown') + b'\0' * 8 + struct.pack('>II', 20, 0)).hex()
    group_items = [(ref('livingworld_entitycategories', 'ped_class_jock') + b'\0' * 8 + struct.pack('>fI', 0.25, 0)).hex()]
    category_items = [(ref('livingworld_entities', 'jock01') + b'\0' * 8).hex(),
                      (ref('livingworld_entities', 'skater_female') + b'\0' * 8).hex()]
    return [
        row('livingworld_census', 'pedestrians', **{'Hash_98F6489E23ED2661': {
            'type': 'Sk8::LivingWorld::tLWCensusEntry', 'data': census_entry}}),
        row('livingworld_census', 'downtown', 'pedestrians',
            Hash_4936C5A55AB38AF3={'type': 'EA::Reflection::Float', 'data': f32(6.5)}),
        row('livingworld_categorygroups', 'downtown', **{'Hash_D5E1267E2D715124': {
            'type': 'Sk8::LivingWorld::tLWGroupEntry', 'data': '', 'array': {'items': group_items}}}),
        row('livingworld_entitycategories', 'ped_class_jock', **{'Hash_D5E1267E2D715124': {
            'type': 'Attrib::RefSpec', 'data': '', 'array': {'items': category_items}}}),
        row('livingworld_entities', 'jock01', Model={'type': 'Attrib::RefSpec',
                                                     'data': (ref('livingworld_models', 'jock01') + b'\0' * 8).hex()}),
        row('livingworld_entities', 'skater_female', Model={'type': 'Attrib::RefSpec',
                                                            'data': (ref('livingworld_models', 'skater_female') + b'\0' * 8).hex()}),
        row('livingworld_models', 'census_spawned'),
        row('livingworld_models', 'jock01', 'census_spawned',
            Hash_62F7C585686F3371={'type': 'EA::Reflection::Text', 'data': 'male_jock_1'}),
        row('livingworld_models', 'skater_female', 'census_spawned'),
        row('livingworld_models', 'skater_female01', 'skater_female',
            Hash_62F7C585686F3371={'type': 'EA::Reflection::Text', 'data': 'female_skater_1'}),
        row('livingworld_census_ranges', 'pedestrians', Hash_E0C6647F889A71FC={
            'type': 'Sk8::LivingWorld::tLWCensusCircle', 'data': struct.pack('>5f', 50, 60, 70, 0, 45).hex()}),
        row('livingworld_entity_animation', 'default', Hash_74C0B72217A7704E={
            'type': 'Sk8::LivingWorld::tRecoveryCollisionIntervals', 'data': struct.pack('>2f', 2.0, 3.167).hex()}),
    ]


def recipe_bytes(name='male_test_1', materials=1):
    def text(s):
        return struct.pack('>I', len(s)) + s.encode()
    out = struct.pack('>I', 7) + text(name) + struct.pack('>III', 1, 15, 0) + struct.pack('>I', 1)
    out += text('Rostral') + struct.pack('>IQI', 1, 0x1122, 2)
    for model in (0x13F403E38811, 0x13F503E38811):
        out += struct.pack('>QBQII', 0xAB, 7, model, 1, materials)
        if materials:
            out += struct.pack('>QI', 0x314103E38817, 2) + text('diffuse') + struct.pack('>Q', 0x1F9703E38818)
            out += text('normal') + struct.pack('>Q', 0x1F9803E38818)
    out += b'\0' * (-len(out) % 4)
    return out + struct.pack('>I', len(out))


class Tables(unittest.TestCase):
    def test_inheritance_structs_refs_and_readable_names(self):
        doc = lw.tables(collections())
        census = doc['classes']['livingworld_census']['downtown']
        self.assertEqual(census['parent'], 'pedestrians')
        self.assertEqual(census['fields']['entry'],
                         {'group': {'class': 'livingworld_categorygroups', 'key': 'downtown'}, 'max_population': 20})
        self.assertEqual(census['fields']['Hash_4936C5A55AB38AF3'], 6.5)
        circle = doc['classes']['livingworld_census_ranges']['pedestrians']['fields']['circle_slow']
        self.assertEqual(circle, {'spawn_inner': 50.0, 'spawn_outer': 60.0, 'cull': 70.0, 'forward_offset': 0.0,
                                  'speed_kmh': 45.0})
        anim = doc['classes']['livingworld_entity_animation']['default']['fields']['recovery_intervals']
        self.assertEqual(anim, {'lying_down_until': 2.0, 'crouched_until': 3.167})
        self.assertEqual(doc['field_names']['livingworld_census']['entry'], 'Hash_98F6489E23ED2661')

    def test_census_chain_resolves_group_model_records_to_their_children(self):
        chain = lw.census_chain(lw.tables(collections()), 'downtown')
        self.assertEqual(chain['max_population'], 20)
        self.assertEqual(chain['categories'][0]['weight'], 0.25)
        recipes = {e['entity']: e['recipes'] for e in chain['categories'][0]['entities']}
        self.assertEqual(recipes, {'jock01': ['male_jock_1'], 'skater_female': ['female_skater_1']})

    def test_empty_refspec_is_none(self):
        refs = lw.Refs(collections())
        self.assertIsNone(refs.ref(ref('livingworld_models', '')))


class CensusGrid(unittest.TestCase):
    def layer(self):
        # One 16 x 16 m tile at the origin: quadrant (-x, -z) holds key 0, (+x, +z) key 1, the rest unpainted.
        leaf = lambda value: [0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, value]  # noqa: E731
        nodes = [[1, 2, 3, 4, 0xFFFF], leaf(0), leaf(0xFFFF), leaf(0xFFFF), leaf(1)]
        return {'layer': 'livingworld_npc_census', 'box': [0.0, 0.0, 8.0, 8.0], 'nodes': nodes,
                'keys': [hash64('aletown'), hash64('mall')]}

    def test_grid_round_trip_and_lookup(self):
        names = lw.census_record_names(['aletown', 'mall'])
        grid = lw.census_grid({'livingworld_npc_census': [self.layer()]}, names, cell=4.0)
        self.assertEqual(grid['size'], [4, 4])
        data = lw.write_census_grid(grid)
        back = lw.read_census_grid(data)
        self.assertEqual(back['names'], grid['names'])
        self.assertEqual(lw.census_at(back, 'livingworld_npc_census', -6, -6), 'aletown')
        self.assertEqual(lw.census_at(back, 'livingworld_npc_census', 6, 6), 'mall')
        self.assertIsNone(lw.census_at(back, 'livingworld_npc_census', -6, 6))
        self.assertIsNone(lw.census_at(back, 'livingworld_npc_census', 100, 0))
        self.assertIsNone(lw.census_at(back, 'livingworld_vehicle_census', -6, -6))

    def test_bad_magic_is_rejected(self):
        with self.assertRaises(ValueError):
            lw.read_census_grid(b'NOTAGRID' + b'\0' * 64)


class Streams(unittest.TestCase):
    def test_road_segment_table(self):
        head = struct.pack('>8f', -10, 0, -10, 1, 10, 5, 10, 1) + struct.pack('>6I', 0, 1, 1, 0xDD000000, 0x80, 0x40)
        head += b'\xde' * (0x40 - len(head))
        segment = struct.pack('>QQI', 0xA1, 0xB1, 3) + b'\xde' * 4 + struct.pack('>QIffffII', 0xB2, 0, 143.5, 8.0, 0.0,
                                                                                  14.1666, 2, 2) + b'\xde' * 4
        parsed = lw.parse_roads(head + segment)
        self.assertEqual(parsed['tables']['intersections'], None)
        self.assertEqual(parsed['bbox'], [[-10.0, 0.0, -10.0], [10.0, 5.0, 10.0]])
        s = parsed['segments'][0]
        self.assertEqual((s['id'], s['node_a'], s['node_b'], s['length'], s['width_a']),
                         ('00000000000000A1', '00000000000000B1', '00000000000000B2', 143.5, 8.0))
        with self.assertRaises(ValueError):
            lw.parse_roads(head)  # segment table past the end

    def test_waypoint_group(self):
        strings = b'loc|WP_1\0waypointgroup\0waypoint\0'
        group_at, wp_at, str_at = 0x20, 0x80, 0xB0
        blob = struct.pack('>6I', 1, 1, 3, group_at, wp_at, str_at) + b'\0' * 8
        blob += struct.pack('>12f', 1, 2, 3, 0, 0, 1, 2, 0, 2, 3, 4, 0)
        blob += struct.pack('>4Q', 7, 8, 9, hash64('waypoint_usetrashbin'))
        blob += struct.pack('>4I', 1, wp_at, str_at, str_at + 9)
        blob += struct.pack('>8f', 1, 2, 3, 0, 1, 0, 0, 0) + struct.pack('>4I', 0, group_at, str_at, str_at + 23)
        blob += strings
        groups = lw.parse_waypoints(blob, {hash64(n): n for n in lw.WAYPOINT_TYPES})
        self.assertEqual(groups[0]['type'], 'waypoint_usetrashbin')
        self.assertEqual(groups[0]['class'], 'waypointgroup')
        self.assertEqual(groups[0]['waypoints'][0]['locator'], 'loc|WP_1')
        self.assertEqual(groups[0]['waypoints'][0]['position'], [1.0, 2.0, 3.0])

    def test_pack_round_trip(self):
        tiles = [(1, 'cSim_0_0_high', b'abc'), (2, 'cSim_1_0_high', b'x' * 33)]
        self.assertEqual(lw.read_pack(lw.write_pack(lw.ROADS_MAGIC, tiles), lw.ROADS_MAGIC), tiles)


class Recipes(unittest.TestCase):
    def test_binary_recipe(self):
        recipe = lw.parse_recipe(recipe_bytes())
        self.assertEqual(recipe['name'], 'male_test_1')
        part = recipe['parts'][0]
        self.assertEqual(part['slot'], 'Rostral')
        self.assertEqual([l['model'] for l in part['lods']], ['000013f403e38811', '000013f503e38811'])
        self.assertEqual(part['lods'][0]['textures'], {'diffuse': '00001f9703e38818', 'normal': '00001f9803e38818'})

    def test_lod_without_material_has_no_texture_list(self):
        recipe = lw.parse_recipe(recipe_bytes(materials=0))
        self.assertIsNone(recipe['parts'][0]['lods'][1]['material'])
        self.assertEqual(recipe['parts'][0]['lods'][1]['textures'], {})

    def test_truncated_or_wrong_version_is_rejected(self):
        data = recipe_bytes()
        with self.assertRaises((ValueError, struct.error)):
            lw.parse_recipe(data[:-6])
        with self.assertRaises(ValueError):
            lw.parse_recipe(struct.pack('>I', 6) + data[4:])


class Validation(unittest.TestCase):
    def write(self, root, recipes):
        out = root/'living_world'
        out.mkdir(parents=True)
        (out/'tables.json').write_text(json.dumps(lw.tables(collections())))
        (out/'models.json').write_text(json.dumps({'recipes': {r: {'missing': []} for r in recipes}}))
        grid = lw.census_grid({'livingworld_npc_census': [CensusGrid().layer()]},
                              lw.census_record_names(['aletown', 'mall']))
        (out/'Test.census.bin').write_bytes(lw.write_census_grid(grid))
        (out/'census.json').write_text(json.dumps({'districts': {'Test': {'file': 'Test.census.bin', 'npc_records': []}}}))

    def test_complete_export_validates(self):
        with tempfile.TemporaryDirectory() as temp:
            self.write(Path(temp), ['male_jock_1', 'female_skater_1'])
            self.assertEqual(lw.validate(Path(temp)), [])

    def test_missing_recipe_is_reported(self):
        with tempfile.TemporaryDirectory() as temp:
            self.write(Path(temp), ['male_jock_1'])
            problems = lw.validate(Path(temp))
            self.assertTrue(any('female_skater_1' in p for p in problems), problems)

    def test_missing_tables_are_reported(self):
        with tempfile.TemporaryDirectory() as temp:
            self.assertTrue(lw.validate(Path(temp))[0].startswith('living world tables do not load'))


class Models(unittest.TestCase):
    def test_skinned_two_lod_glb_from_a_recipe(self):
        from PIL import Image
        from tools.asset_pipeline import living_world_models as models
        eye = [[1.0, 0, 0, 0], [0, 1.0, 0, 0], [0, 0, 1.0, 0], [0, 0, 0, 1.0]]
        up = [[1.0, 0, 0, 0], [0, 1.0, 0, 0], [0, 0, 1.0, 0], [0, 1.0, 0, 1.0]]
        bones = [{'name': 'Hips', 'parent': -1, 'bind_matrix': eye},
                 {'name': 'Spine', 'parent': 0, 'parent_name': 'Hips', 'bind_matrix': up},
                 {'name': 'HeadEnd', 'parent': -1, 'bind_matrix': up}]  # stored without a parent
        mesh = {'positions': [(0, 0, 0), (1, 0, 0), (0, 1, 0)], 'uvs': [(0, 0), (1, 0), (0, 1)],
                'indices': [0, 1, 2], 'skin_bones': [(0, 0, 0, 1)] * 3,
                'skin_weights': [(0.0, 0.0, 0.0, 1.0)] * 3}

        class FakeRx2:
            @staticmethod
            def parse_rx2(path):
                return {'bones': bones, 'meshes': [mesh]}
        recipe = lw.parse_recipe(recipe_bytes())
        material = next(l['material'] for p in recipe['parts'] for l in p['lods'] if l['material'])
        recipe['shaders'] = {material: 'pedestrian_high_stamp'}
        with tempfile.TemporaryDirectory() as temp:
            png = Path(temp)/'t.png'
            Image.new('RGBA', (4, 4), (10, 20, 30, 255)).save(png)
            out = Path(temp)/'m.glb'
            stats = models.write_glb(recipe, lambda lod: Path(temp)/'x.rx2', lambda tid: png, out, FakeRx2)
            doc = models.read_glb(out)
        self.assertEqual(stats['bones'], 3)
        self.assertEqual([m['name'] for m in doc['meshes']], ['LOD0', 'LOD1'])
        self.assertEqual(len(doc['skins'][0]['joints']), 3)
        names = {n['name']: n for n in doc['nodes']}
        self.assertEqual(names['Hips'].get('children'), [1])
        self.assertIn(2, names['male_test_1']['children'])  # parentless bone hangs off the rig root
        self.assertEqual(names['Spine']['matrix'][13], 1.0)  # local = world relative to Hips
        self.assertEqual(len(doc['materials']), 1)  # both LODs share the material
        self.assertIn('normalTexture', doc['materials'][0])
        self.assertEqual(doc['materials'][0].get('extras'), {'shader': 'pedestrian_high_stamp'})

    def test_recipe_xml_material_types(self):
        xml = (b'<compositeasset n="x"><mat id="0x0000313703E38817" type="pedestrian_high_stamp">'
               b'<sp id="0x1" chn="diffuse" /></mat> <mat id="0x311f03e38817" type="marquee_hair"></mat>'
               b'</compositeasset>')
        self.assertEqual(lw.recipe_shaders(xml), {'0000313703e38817': 'pedestrian_high_stamp',
                                                  '0000311f03e38817': 'marquee_hair'})


class Group(unittest.TestCase):
    def test_livingworld_is_a_setup_group_with_both_exporters_in_its_fingerprint(self):
        from tools.asset_pipeline import versions, group_receipts
        self.assertIn('livingworld', versions.GROUPS)
        sources = versions.SOURCES['livingworld']
        for name in ('asset_pipeline/living_world.py', 'asset_pipeline/living_world_skaters.py',
                     'asset_pipeline/vlt.py', 'asset_pipeline/names.txt'):
            self.assertIn(name, sources)
        self.assertIn('livingworld', group_receipts.ROOTS)
        self.assertIn('livingworld', versions.fingerprints())


if __name__ == '__main__':
    unittest.main()
