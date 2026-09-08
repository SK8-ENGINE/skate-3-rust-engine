from pathlib import Path
import json
import struct
import tempfile
import unittest

from tools.asset_pipeline.original_content import extract
from tools.owned_game.big import BigArchive


def big4(entries):
    end = 16 + sum(9 + len(name.encode()) for name, _ in entries)
    size = end + sum(len(data) for _, data in entries)
    directory, payload, offset = bytearray(), bytearray(), end
    for name, data in entries:
        directory += struct.pack('>II', offset, len(data)) + name.encode() + b'\0'
        payload += data
        offset += len(data)
    return b'BIG4' + struct.pack('<I', size) + struct.pack('>II', len(entries), end) + directory + payload


class OriginalContentTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.game, self.out = self.root/'game', self.root/'output'
        self.game.mkdir()

    def test_nested_banks_duplicate_names_loose_files_and_repair(self):
        (self.game/'a.big').write_bytes(big4([('same', b'a'), ('nested.big', big4([('child', b'c')]))]))
        (self.game/'b.big').write_bytes(big4([('same', b'b')]))
        (self.game/'loose.bin').write_bytes(b'original')
        first = extract(self.game, self.out, lambda _: None)
        self.assertEqual(len(first['files']), 5)
        self.assertEqual((self.out/'banks/a.big/same').read_bytes(), b'a')
        self.assertEqual((self.out/'banks/b.big/same').read_bytes(), b'b')
        self.assertEqual((self.out/'banks/a.big/nested.big.contents/child').read_bytes(), b'c')
        target = self.out/'banks/a.big/same'
        stamp = target.stat().st_mtime_ns
        self.assertEqual(extract(self.game, self.out, lambda _: None), first)
        self.assertEqual(stamp, target.stat().st_mtime_ns)
        target.write_bytes(b'x')
        extract(self.game, self.out, lambda _: None)
        self.assertEqual(target.read_bytes(), b'a')

    def test_big4_refpack_and_invalid_offsets(self):
        # Header, literal of three bytes, end marker.
        path = self.game/'compressed.big'
        path.write_bytes(big4([('data', b'\x10\xfb\x00\x00\x03\xffabc')]))
        archive = BigArchive(path)
        self.assertEqual(archive.read(archive.entries[0]), b'abc')
        raw = bytearray(path.read_bytes())
        raw[16:20] = struct.pack('>I', len(raw) + 1)
        path.write_bytes(raw)
        with self.assertRaisesRegex(ValueError, 'bounds'):
            BigArchive(path)

    def test_unsafe_member_and_source_overlap_fail(self):
        (self.game/'bad.big').write_bytes(big4([('../escape', b'bad')]))
        with self.assertRaisesRegex(ValueError, 'unsafe'):
            extract(self.game, self.out, lambda _: None)
        self.assertFalse((self.out/'manifest.json').exists())
        with self.assertRaisesRegex(ValueError, 'separate'):
            extract(self.game, self.game/'output', lambda _: None)

    def test_case_collisions_do_not_publish_completed_index(self):
        (self.game/'bad.big').write_bytes(big4([('Name', b'a'), ('name', b'b')]))
        with self.assertRaisesRegex(ValueError, 'Ambiguous'):
            extract(self.game, self.out, lambda _: None)
        self.assertFalse((self.out/'manifest.json').exists())


if __name__ == '__main__':
    unittest.main()
