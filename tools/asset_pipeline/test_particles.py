"""Particle sprite names, independent of owned game assets."""
import unittest

from .particles import texture_names


class TextureNameTests(unittest.TestCase):
    def test_names_follow_file_order_and_are_lowercased(self):
        raw = b'\x00steam.Texture\x00junk\x00Dust.Texture\x00water.Texture\x00'
        self.assertEqual(texture_names(raw), ['steam', 'dust', 'water'])

    def test_no_names(self):
        self.assertEqual(texture_names(b'RW4xb2 no names here'), [])


if __name__ == '__main__':
    unittest.main()
