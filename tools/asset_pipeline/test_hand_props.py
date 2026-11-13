import unittest

from tools.asset_pipeline.hand_props import model_ids


class HandPropModelIds(unittest.TestCase):
    def test_joins_handprops_to_dmo_models_and_reads_the_u64_forms(self):
        tables = {'classes': {
            'livingworld_handprops': {
                'pop': {'parent': 'drinks', 'fields': {'model': {'class': 'dmo_models', 'key': 'popcan_lw'}}},
                'orange': {'parent': None, 'fields': {'model': {'class': 'dmo_models', 'key': 'orange_lw'}}},
                'nomodel': {'parent': None, 'fields': {}},
                'missing': {'parent': None, 'fields': {'model': {'class': 'dmo_models', 'key': 'gone_lw'}}},
            },
            'dmo_models': {
                'popcan_lw': {'parent': 'default', 'fields': {'Hash_73061AB91FDAE615': {'type': 'EA::Reflection::UInt64', 'hex': '0001991803E38707'}}},
                'orange_lw': {'parent': 'default', 'fields': {'Hash_73061AB91FDAE615': '0001991503e38707'}},
            },
        }}
        ids = model_ids(tables)
        self.assertEqual(ids['pop'], {'model': 'popcan_lw', 'template': '0001991803E38707'})
        self.assertEqual(ids['orange']['template'], '0001991503E38707')
        self.assertEqual(ids['missing'], {'model': 'gone_lw', 'template': None})
        self.assertNotIn('nomodel', ids)


if __name__ == '__main__':
    unittest.main()
