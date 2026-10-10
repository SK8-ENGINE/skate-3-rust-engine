"""Living world vehicles and the road network v2, checked on synthetic data (never game data)."""
import struct
import tempfile
import unittest
from pathlib import Path

import numpy as np

from tools.asset_pipeline import living_world as lw
from tools.asset_pipeline import living_world_roads as roads
from tools.asset_pipeline import living_world_vehicles as veh
from tools.asset_pipeline.vlt import hash64

J, CONN, ARC_C, LISTS, SEG, RUN, PIECES, ARC_P, RUN_LIST = 0x40, 0x2A0, 0x310, 0x350, 0x360, 0x3A0, 0x440, 0x600, 0x680


def _vec(x, y, z, w=0.0):
    return struct.pack('>4f', x, y, z, w)


def _end(rid, kind, side, lanes, counts=(0, 0, 0, 0), offsets=(0, 0, 0, 0)):
    return struct.pack('>QIII4I4I', rid, kind, side, lanes, *counts, *offsets) + b'\xde' * 4


def _curve(start, end, length, arc_offset):
    tangent = tuple(e - s for s, e in zip(start, end))
    return _vec(*start) + _vec(*end) + _vec(*tangent) + _vec(*tangent) + struct.pack('>fII', length, 16, arc_offset) + b'\xde' * 4


def _arc(length):
    return struct.pack('>16f', *[length * k / 15 for k in range(16)])


def road_object():
    """One tile: junction node 0xA (approach end 0 lane 0 -> connector 0 -> exit end 2 lane 0), segment 1
    from node 0xB into the junction at end 0, its lane run with two 4 m pieces."""
    blob = bytearray(0x690)
    blob[0:0x40] = struct.pack('>8f', 0, 0, -5, 1, 24, 1, 5, 1) + struct.pack('>6I', 1, 1, 1, J, RUN, SEG) + b'\xde' * 8
    j = bytearray()
    for quad, tag in ((((8, 0, -2), (12, 0, -2), (12, 0, 2), (8, 0, 2)), 7), (((8, 0, -6), (20, 0, -6), (20, 0, 6), (8, 0, 6)), 9)):
        for corner in quad:
            j += struct.pack('>3fI', *corner, tag)
    for slot in range(8):
        at = J + 0x80 + 0x38 * slot
        if slot == 0:
            j += _end(0x10, 1, 1, 1, (1, 0, 0, 0), (LISTS - at, 0, 0, 0))
        elif slot == 6:
            j += _end(0x11, 1, 0, 1, (1, 0, 0, 0), (LISTS + 4 - at, 0, 0, 0))
        else:
            j += _end(0, 2, 0xFFFFFFFF, 2)
    j += struct.pack('>fIQII', 13.888889, 1, 0xA, CONN - J, 1) + b'\xde' * 8
    blob[J:J + len(j)] = j
    c = _curve((8, 0, 0), (20, 0, 0), 12.0, ARC_C - CONN) + struct.pack('>f4I', 13.888889, 0, 0, 2, 0) + b'\xde' * 12
    blob[CONN:CONN + 0x70] = c
    blob[ARC_C:ARC_C + 64] = _arc(12.0)
    blob[LISTS:LISTS + 8] = struct.pack('>II', 0, 0)
    seg = struct.pack('>QQI', 1, 0xA, 0) + b'\xde' * 4 + struct.pack('>QIffffII', 0xB, 1, 8.0, 4.0, 4.0, 14.166667, 1, 3) + b'\xde' * 4
    blob[SEG:SEG + 0x40] = seg
    run = struct.pack('>Q', 0x77) + _end(0x55, 1, 0, 1) + _end(0xA, 0, 0, 1, (1, 0, 0, 0), (RUN_LIST - (RUN + 0x40), 0, 0, 0))
    run += struct.pack('>IIfIfffIQ', 2, PIECES - RUN, 14.166667, 1, 4.0, 8.0, 8.0, 0, 1)
    blob[RUN:RUN + 0xA0] = run
    for k in range(2):
        at = PIECES + 0xE0 * k
        piece = _curve((4 * k, 0, 0), (4 * k + 4, 0, 0), 4.0, ARC_P + 64 * k - at)
        piece += _vec(4 * k, 0, 2) + _vec(4 * k, 0, -2) + _vec(4 * k + 4, 0, 2) + _vec(4 * k + 4, 0, -2)
        piece += _vec(4, 0, 0) * 4 + struct.pack('>f', 4.0 * (k + 1)) + b'\xde' * 12
        blob[at:at + 0xE0] = piece
        blob[ARC_P + 64 * k:ARC_P + 64 * (k + 1)] = _arc(4.0)
    blob[RUN_LIST:RUN_LIST + 4] = struct.pack('>I', 0)
    return bytes(blob)


class Roads(unittest.TestCase):
    def test_object_decodes_junction_connector_and_lane_run(self):
        d = roads.decode_object(road_object())
        (j,) = d['junctions']
        self.assertEqual((j['node'], j['speed'], j['flag_04'], j['quad_tags']), ('000000000000000A', 13.888889, 1, [7, 9]))
        self.assertEqual(j['approaches'][0]['connectors'], [[0]])
        self.assertEqual(j['exits'][2]['connectors'], [[0]])
        self.assertEqual(j['approaches'][1]['kind'], 2)
        (c,) = j['connectors']
        self.assertEqual((c['from_end'], c['from_lane'], c['to_end'], c['to_lane'], c['length']), (0, 0, 2, 0, 12.0))
        self.assertEqual(c['arc'][-1], 12.0)
        (run,) = d['runs']
        self.assertEqual((run['segment'], run['lanes'], run['first_piece'], len(run['pieces'])), ('0000000000000001', 1, 0, 2))
        self.assertEqual(run['end']['connectors'], [[0]])
        self.assertEqual(run['pieces'][1]['end'], [8.0, 0.0, 0.0])

    def test_build_merges_tile_copies_and_names_the_destination(self):
        graph = roads.build([road_object(), road_object()])
        self.assertEqual(graph['problems'], [])
        s = graph['segments']['0000000000000001']
        self.assertEqual((s['to_node'], s['to_end'], s['from_node'], s['from_end']), ('000000000000000A', 0, '000000000000000B', 1))
        self.assertTrue(s['pieces_complete'])
        self.assertEqual([p['distance'] for p in s['pieces']], [4.0, 8.0])
        self.assertEqual(list(graph['junctions']), ['000000000000000A'])

    def test_incomplete_pieces_are_reported(self):
        blob = bytearray(road_object())
        struct.pack_into('>I', blob, RUN + 0x78, 1)  # only the first piece
        struct.pack_into('>f', blob, RUN + 0x88, 4.0)  # run length 4
        graph = roads.build([bytes(blob)])
        self.assertFalse(graph['segments']['0000000000000001']['pieces_complete'])
        self.assertTrue(any('incomplete' in p for p in graph['problems']))

    def test_graph_round_trip_and_size_check(self):
        data = roads.write_graph({'Test': roads.build([road_object()])})
        self.assertEqual(data[:8], b'LWROADS2')
        back = roads.read_graph(data)
        self.assertEqual(back['districts'], [('Test', 0, 1, 0, 1)])
        self.assertEqual((len(back['segments']), len(back['pieces']), len(back['junctions']), len(back['connectors'])), (1, 2, 1, 1))
        self.assertEqual(back['lanes'], [0, 0])
        self.assertEqual(back['segments'][0][:3], (1, 0xA, 0xB))
        self.assertEqual(roads.write_graph({'Test': roads.build([road_object()])}), data, 'deterministic')
        with self.assertRaises(ValueError):
            roads.read_graph(data[:-4])

    def test_segment_count_is_the_third_header_word(self):
        blob = bytearray(road_object())
        struct.pack_into('>I', blob, 0x24, 2)  # two lane runs, one segment (University cSim_150_250 shape)
        self.assertEqual(len(lw.parse_roads(bytes(blob))['segments']), 1)

    def test_bad_offsets_are_rejected(self):
        blob = bytearray(road_object())
        struct.pack_into('>I', blob, J + 0x240 + 20, 999)  # connector count
        with self.assertRaises(ValueError):
            roads.decode_object(bytes(blob))


def recipe_bytes(instances=2):
    def text(s):
        return struct.pack('>I', len(s)) + s.encode()
    out = struct.pack('>I', 7) + text('car_test') + struct.pack('>III', 2, 15, 0) + struct.pack('>I', 1)
    out += text('Misc') + struct.pack('>IQI', 1, 0x55, 1) + struct.pack('>QBQI', 0xAB, 3, 0x148B03E38811, instances)
    for _ in range(instances):
        out += struct.pack('>IQI', 1, 0x31A903E38817, 1) + text('diffuse') + struct.pack('>Q', 0x200C03E38818)
    out += b'\0' * (-len(out) % 4)
    return out + struct.pack('>I', len(out))


class Recipes(unittest.TestCase):
    def test_lod_word_is_the_material_instance_count(self):
        recipe = lw.parse_recipe(recipe_bytes(2))
        lod = recipe['parts'][0]['lods'][0]
        self.assertEqual((lod['word'], lod['material'], len(lod['instances'])), (2, '000031a903e38817', 2))
        single = lw.parse_recipe(recipe_bytes(1))['parts'][0]['lods'][0]
        self.assertNotIn('instances', single)  # ped / car LODs with one instance keep the old shape

    def test_material_types_from_the_xml_sibling(self):
        xml = b'<compositeasset><mat id="0x000031a303e38817" type="vehicle_glass"></mat>' \
              b'<mat id="0x000031a903e38817" type="vehicle_chassis"></mat></compositeasset>'
        self.assertEqual(veh.material_types(xml), {'000031a303e38817': 'vehicle_glass', '000031a903e38817': 'vehicle_chassis'})


class GrabSplines(unittest.TestCase):
    def test_the_first_part_arena_with_grabdata_gives_the_splines(self):
        from tools.asset_pipeline.grab_data import GRABDATA
        from tools.asset_pipeline.test_dynamic_props import resource
        from tools.asset_pipeline.test_grab_data import grabdata
        points = [(-0.8, 0.9, -1.7), (-0.8, 0.9, -1.7), (0.8, 0.9, -1.7), (0.8, 0.9, -1.7)]
        arenas = {'body': resource(0x2F0000, b'\0' * 16), 'acc': resource(GRABDATA, grabdata(points)),
                  'later': resource(GRABDATA, grabdata([(0.0, 0.0, 0.0)] * 4))}
        recipe = {'parts': [{'lods': [{'arena': 'body'}]}, {'lods': [{'arena': 'acc'}]}, {'lods': [{'arena': 'later'}]}]}
        found = veh.recipe_grab_splines(recipe, arenas.__getitem__)
        self.assertEqual(len(found), 1)
        np.testing.assert_allclose(found[0]['points'][2], points[2], rtol=1e-6)
        self.assertEqual(found[0]['direction'], [0.0, 0.0, -1.0])
        self.assertEqual(veh.recipe_grab_splines({'parts': [{'lods': [{'arena': 'body'}]}]}, arenas.__getitem__), [])


class FakeRx2:
    TYPE_RAW_BUFFER = 0x10031

    def __init__(self, sections):
        self.sections = sections

    def parse_header(self, data):
        return {}

    def parse_sections(self, data, header):
        return self.sections


class Positions(unittest.TestCase):
    def test_half_float_positions_from_the_vertex_buffer(self):
        points = np.array([[1.5, 0.25, -2.0], [0.0, 1.0, 4.5]], dtype=np.float64)
        stride = 12
        buffer = b''
        for p in points:
            buffer += np.array([*p, 1.0], dtype='>f2').tobytes() + b'\x11\x22\x33\x44'
        data = b'\0' * 32 + buffer
        mesh = {'stride': stride, 'vertex_count': 2, 'vb_padding': 0,
                'vertex_elements': [{'usage_name': 'POSITION', 'format': veh.FLOAT16_4, 'offset': 0}]}
        rx2 = FakeRx2([{'type_code': 0x10031, 'offset': 32, 'size': len(buffer), 'file_offset': 32}])
        np.testing.assert_allclose(veh.half_positions(data, mesh, rx2), points)
        mesh['vertex_elements'][0]['format'] = 0x1A215A
        with self.assertRaises(ValueError):
            veh.half_positions(data, mesh, rx2)


def vehicle_tables():
    rgba = lambda r, g, b: {'x': r, 'y': g, 'z': b, 'w': 1.0}  # noqa: E731
    ref = lambda cls, key: {'class': cls, 'key': key}  # noqa: E731
    return {'version': 1, 'classes': {
        'livingworld_census': {'dwntwn': {'parent': None, 'fields': {
            'entry': {'group': ref('livingworld_categorygroups', 'dwntwn'), 'max_population': 30}, 'vehicle_extra': 20}}},
        'livingworld_categorygroups': {'dwntwn': {'parent': None, 'fields': {'categories': [
            {'category': ref('livingworld_entitycategories', 'taxis'), 'weight': 0.1}]}}},
        'livingworld_entitycategories': {'taxis': {'parent': None, 'fields': {'entities': [ref('livingworld_entities', 'taxi01')]}}},
        'livingworld_entities': {'taxi01': {'parent': 'taxi', 'fields': {
            'model': ref('livingworld_models', 'vehicle_taxi01'), 'spec': ref('livingworld_vehicle_characteristics', 'vehicle_spec_taxi01'),
            'driver': ref('livingworld_vehicle_drivers', 'driver_taxi'), 'scoring': ref('scoring_entities', 'car'),
            'ai_graph': 'state/livingworldentities/vehicle/Vehicle.xml'}}},
        'livingworld_models': {'vehicle_taxi01': {'parent': 'vehicle_taxi', 'fields': {
            'recipe': 'taxi_sedan_01', 'category': 3, 'chassis_colours': [rgba(1.0, 0.82, 0.15), rgba(0.94, 0.19, 0.16)],
            'secondary_colours': [rgba(0.22, 0.22, 0.22)], 'Hash_F983F2518B335286': {'x': 1.9, 'y': 1.9, 'z': 4.5},
            'Hash_FD7A66142F16B9CC': 0.322}}},
        'livingworld_vehicle_characteristics': {'vehicle_spec_taxi01': {'parent': 'default', 'fields': {}}},
        'livingworld_vehicle_drivers': {'driver_taxi': {'parent': 'default', 'fields': {}}},
    }}


class VehicleDoc(unittest.TestCase):
    def manifest(self):
        return {'recipes': {'taxi_sedan_01': {'parts': [{'slot': 'Accessory', 'lods': [{}]}, {'slot': 'Equipment', 'lods': [{}]}],
                                              'material_types': {'01': 'vehicle_chassis'},
                                              'glb': {'status': 'ready', 'file': 'vehicles/taxi_sedan_01.glb',
                                                      'bounds': [[-1, 0, -2.6], [1, 1.6, 2.7]], 'wheels': {}}}}}

    def test_palettes_entities_and_census_resolve(self):
        doc = vehicle_tables()
        v = veh.vehicle_doc(doc, self.manifest(), ['dwntwn'])
        model = v['models']['vehicle_taxi01']
        self.assertEqual(model['chassis_colours'][0], [1.0, 0.82, 0.15, 1.0])
        self.assertEqual(model['palette_ids']['chassis'], ['vehicle_taxi01/chassis/0', 'vehicle_taxi01/chassis/1'])
        self.assertEqual(model['palette_ids']['secondary'], ['vehicle_taxi01/secondary/0'])
        self.assertEqual(v['entities']['taxi01']['driver'], 'driver_taxi')
        self.assertEqual(v['entities']['taxi01']['recipe'], 'taxi_sedan_01')
        self.assertEqual(v['census']['dwntwn']['categories'], [{'category': 'taxis', 'weight': 0.1, 'entities': ['taxi01']}])
        self.assertEqual(v['recipes']['taxi_sedan_01']['parts'][1]['role'], 'windows')
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp)/'vehicles').mkdir()
            (Path(tmp)/'vehicles'/'taxi_sedan_01.glb').write_bytes(b'glTF')
            self.assertEqual(veh.validate(doc, v, Path(tmp)), [])
            doc['classes']['livingworld_vehicle_drivers'] = {}
            self.assertTrue(any('driver driver_taxi missing' in p for p in veh.validate(doc, v, Path(tmp))))

    def test_unbuilt_car_is_a_problem(self):
        doc = vehicle_tables()
        v = veh.vehicle_doc(doc, self.manifest(), ['dwntwn'])
        with tempfile.TemporaryDirectory() as tmp:
            self.assertTrue(any('not built' in p for p in veh.validate(doc, v, Path(tmp))))


class Tables(unittest.TestCase):
    def test_vehicle_classes_and_names_are_exported(self):
        def row(cls, key, parent='', **fields):
            return {'class': cls, 'key': key, 'parent': parent, 'fields': fields}
        f32 = lambda v: struct.pack('>f', v).hex().upper()  # noqa: E731
        ref = struct.pack('>QQ', hash64('livingworld_vehicle_drivers'), hash64('driver_taxi')) + b'\0' * 8
        doc = lw.tables([
            row('livingworld_vehicle_drivers', 'driver_taxi', **{'Hash_988BB0F6F043EB3D': {'type': 'EA::Reflection::Float', 'data': f32(20.0)}}),
            row('livingworld_entities', 'taxi01', **{'Hash_023EF929823A3C50': {'type': 'Attrib::RefSpec', 'data': ref.hex()}}),
        ])
        self.assertEqual(doc['classes']['livingworld_vehicle_drivers']['driver_taxi']['fields'], {'parked_time': 20.0})
        self.assertEqual(doc['classes']['livingworld_entities']['taxi01']['fields']['driver'],
                         {'class': 'livingworld_vehicle_drivers', 'key': 'driver_taxi'})
        self.assertEqual(doc['field_names']['livingworld_vehicle_drivers']['parked_time'], 'Hash_988BB0F6F043EB3D')

    def test_vehicle_modules_are_in_the_livingworld_fingerprint_only(self):
        from tools.asset_pipeline import versions
        for name in ('asset_pipeline/living_world_roads.py', 'asset_pipeline/living_world_vehicles.py'):
            self.assertIn(name, versions.SOURCES['livingworld'])
            for group, sources in versions.SOURCES.items():
                if group != 'livingworld':
                    self.assertNotIn(name, sources)


if __name__ == '__main__':
    unittest.main()
