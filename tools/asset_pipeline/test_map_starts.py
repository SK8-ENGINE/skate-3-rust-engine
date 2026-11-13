import math
import unittest

from .map_starts import DEFAULT_LOCATIONS, IDENTITY, heading, pick


class MapStartTests(unittest.TestCase):
    def test_heading_matches_game_rotation_convention(self):
        # skate-game: basis = Mat3::from_rotation_y(heading), forward = (sin h, 0, cos h).
        for forward, expected in [((0, 0, 1), 0.), ((1, 0, 0), math.pi / 2),
                                  ((-1, 0, 0), -math.pi / 2), ((0.1536, 0, 0.9881), math.atan2(0.1536, 0.9881))]:
            h = heading(forward)
            self.assertAlmostEqual(h, expected, places=6)
            self.assertAlmostEqual(math.sin(h), forward[0] / math.hypot(forward[0], forward[2]), places=6)
            self.assertAlmostEqual(math.cos(h), forward[2] / math.hypot(forward[0], forward[2]), places=6)

    def test_chosen_defaults_cover_districts_without_a_retail_start(self):
        self.assertEqual(set(DEFAULT_LOCATIONS),
                         {'DIST_University', 'DIST_Industrial', 'DIST_DownTown', 'DIST_MaloofMoneyCup'})

    def test_an_unplaced_world_start_falls_through_to_the_fe_location(self):
        placed = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [-24.4, 51.2, -46.0, 1.]]
        records = {('dist_agpark', 'z_ArtGallery'): [IDENTITY],
                   ('dist_agpark', 'freeskate_artgallery_locator'): [placed]}
        start = pick('DIST_AGPark', [('z_ArtGallery', 'world'), ('freeskate_artgallery_locator', 'fe_locations')],
                     records)
        self.assertEqual((start['locator'], start['position']), ('freeskate_artgallery_locator', [-24.4, 51.2, -46.0]))
        self.assertIsNone(pick('DIST_AGPark', [('z_ArtGallery', 'world'), ('missing', 'fe')], records))


if __name__ == '__main__':
    unittest.main()
