import json
import types
import unittest

from .sky import OVERRIDES, _bind


def fake_prep(groups, group_of_mesh):
    """The GUID binder's shape (prepare_hawaiian_dream): groups per mesh, bindings with the material group index."""
    return types.SimpleNamespace(
        _group_material_parameters=lambda materials: materials,
        _bind_material_groups_by_guid=lambda raw, grouped, count, allow_import_order_fallback: (
            [dict(g) for g in groups], [{'group_index': g} for g in group_of_mesh]),
        _material_metadata=lambda groups, i: {'shader_name': groups[i]['AttribulatorMaterialName'][0],
                                              'channels': sorted(k for k in groups[i] if k != 'AttribulatorMaterialName')},
    )


class SkyTests(unittest.TestCase):
    def test_the_hawaiian_dream_row_turns_material_2_into_alphatest_without_specular(self):
        groups = [{'AttribulatorMaterialName': ['sky.default'], 'diffuse': ['a']},
                  {'AttribulatorMaterialName': ['environment.transparent'], 'diffuse': ['b'], 'specular': ['c']}]
        model = types.SimpleNamespace(materials=[], meshes=[0, 1])
        _, _, materials = _bind(b'', model, fake_prep(groups, [4, 2]), 'WORLD/models/DIST_DW_MegaCompund')
        self.assertEqual(materials[0], {'shader_name': 'sky.default', 'channels': ['diffuse']})
        self.assertEqual(materials[1], {'shader_name': 'environmentsimple.alphatest', 'channels': ['diffuse']})
        # Another model keeps its materials.
        _, _, untouched = _bind(b'', model, fake_prep(groups, [4, 2]), 'world/models/DIST_skybox')
        self.assertEqual(untouched[1]['shader_name'], 'environment.transparent')

    def test_the_override_rows_are_well_formed(self):
        rows = json.loads(OVERRIDES.read_text(encoding='utf-8'))['models']
        for model, row in rows.items():
            for index, change in row['materials'].items():
                self.assertTrue(index.isdigit() and change['shader'] and isinstance(change.get('remove', []), list), model)


if __name__ == '__main__':
    unittest.main()
