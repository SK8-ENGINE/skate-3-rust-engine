import unittest
from types import SimpleNamespace

from .audio_export import bed_table


class Archive:
    def __init__(self, *paths):
        self.entries = [SimpleNamespace(path=p) for p in paths]

    def read(self, entry):
        return entry.path.encode()


class BedTableTest(unittest.TestCase):
    def test_the_disc_index_numbers_the_beds_and_the_first_entry_wins(self):
        headers = Archive('a/06_dt_open.snr', 'a/19_reclaimed_b_fix.snr', 'a/19_reclaimed_b.snr', 'a/readme.txt')
        bodies = Archive('b/06_dt_open.sns', 'b/19_reclaimed_b_fix.sns', 'b/19_reclaimed_b.sns')
        table = bed_table(headers, bodies)
        self.assertEqual({n: s for n, (s, _, _) in table.items()}, {6: '06_dt_open', 19: '19_reclaimed_b_fix'})
        name, (h_archive, head), (b_archive, body) = table[19]
        self.assertEqual((h_archive.read(head), b_archive.read(body)), (b'a/19_reclaimed_b_fix.snr', b'b/19_reclaimed_b_fix.sns'))

    def test_a_disc_bed_without_its_body_is_an_error(self):
        with self.assertRaises(KeyError):
            bed_table(Archive('a/06_dt_open.snr'), Archive())

    def test_a_pack_adds_only_free_numbers_and_needs_both_files(self):
        disc = (Archive('a/06_dt_open.snr'), Archive('b/06_dt_open.sns'))
        after_dark = Archive('data/audio/24_dlc_ambience.snr', 'data/audio/24_dlc_ambience.sns', 'data/audio/cage_rattle_1.abk')
        create = Archive('data/audio/25_dlc_ambience.snr', 'data/audio/25_dlc_ambience.sns',
                         'data/audio/6_dlc_ambience.snr', 'data/audio/6_dlc_ambience.sns',  # a disc number: ignored
                         'data/audio/26_dlc_ambience.snr')  # no body: ignored
        table = bed_table(*disc, packs=[after_dark, create])
        self.assertEqual({n: s for n, (s, _, _) in table.items()},
                         {6: '06_dt_open', 24: '24_dlc_ambience', 25: '25_dlc_ambience'})
        self.assertIs(table[24][1][0], after_dark, 'the pack file, read in place')

    def test_at_most_sixteen_pack_beds(self):
        names = [f'data/audio/{n}_dlc_ambience.{x}' for n in range(30, 50) for x in ('snr', 'sns')]
        table = bed_table(Archive(), Archive(), packs=[Archive(*names)])
        self.assertEqual(len(table), 16)


if __name__ == '__main__':
    unittest.main()
