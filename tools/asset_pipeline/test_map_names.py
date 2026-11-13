import unittest

from .map_names import CATEGORY_LABEL, WORLD_NAME, names, words


def row(cls, key, **fields):
    return {'class': cls, 'key': key, 'parent': '', 'fields': {k: {'data': v} for k, v in fields.items()}}


class MapNameTests(unittest.TestCase):
    def test_category_first_then_the_world_name_then_the_label_in_words(self):
        converted = {'collections': [
            row('world', 'dist_university', WorldStream='DIST_University', **{WORLD_NAME: 'ID_WORLD_UNIV'}),
            row('map_category', 'dist_university', **{CATEGORY_LABEL: 'ID_DISTRICT_UNIVERSITY'}),
            row('world', 'dist_megapark', WorldStream='DIST_MegaPark', **{WORLD_NAME: 'ID_WORLD_SKATEPARK_MEGAPARK'}),
            row('world', 'dist_megapark_empty', WorldStream='DIST_MegaPark', **{WORLD_NAME: 'ID_OTHER'}),
            row('world', 'dist_skateschool', WorldStream='DIST_SkateSchool', **{WORLD_NAME: 'ID_SCHOOL'}),
            row('world', 'dist_maloofmoneycup', WorldStream='DIST_MaloofMoneyCup'),
            row('world', 'dist_sanitariumdlc', WorldStream='DIST_SanitariumDLC', **{WORLD_NAME: 'ID_WORLD_SANITARIUMDLC'}),
            row('map_category', 'sanitariumdlc', **{CATEGORY_LABEL: 'ID_LOCATION_SANITARIUMDLC'}),
        ]}
        text = {'ID_DISTRICT_UNIVERSITY': 'University District', 'ID_WORLD_UNIV': 'Wrong', 'ID_OTHER': 'Wrong',
                'ID_WORLD_SKATEPARK_MEGAPARK': 'Stadium ', 'ID_SCHOOL': '\x8fSkate School',
                'ID_WORLD_SANITARIUMDLC': 'After Dark', 'ID_LOCATION_SANITARIUMDLC': 'The Sanatorium'}
        got = names(converted, [], text)
        self.assertEqual(got['University'], {'name_id': 'ID_DISTRICT_UNIVERSITY', 'name': 'University District'})
        self.assertEqual(got['MegaPark'], {'name_id': 'ID_WORLD_SKATEPARK_MEGAPARK', 'name': 'Stadium'})
        self.assertEqual(got['SkateSchool'], {'name_id': None, 'name': 'Skate School'})
        self.assertEqual(got['MaloofMoneyCup'], {'name_id': None, 'name': 'Maloof Money Cup'})
        self.assertEqual(got['SanitariumDLC'], {'name_id': 'ID_LOCATION_SANITARIUMDLC', 'name': 'The Sanatorium'})

    def test_words(self):
        self.assertEqual(words('DLC_Rob_Fantasy_Plaza'), 'DLC Rob Fantasy Plaza')


if __name__ == '__main__':
    unittest.main()
