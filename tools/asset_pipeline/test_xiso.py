import struct
import tempfile
import unittest
from pathlib import Path

from tools.asset_pipeline.xiso import DIRECTORY, MAGIC, SECTOR, Xiso


def entry(name, sector, size, attributes, left=0, right=0):
    raw = struct.pack('<HHIIBB', left, right, sector, size, attributes, len(name)) + name.encode()
    return raw + b'\xff' * (-len(raw) % 4)


def directory(entries):
    """A directory sector: the entries as a right-leaning tree (each entry's right child is the next one)."""
    blobs, offsets, at = [], [], 0
    for item in entries:
        offsets.append(at)
        at += len(entry(*item))
    for index, item in enumerate(entries):
        right = offsets[index + 1] // 4 if index + 1 < len(entries) else 0
        blobs.append(entry(*item, right=right))
    data = b''.join(blobs)
    return data + b'\xff' * (SECTOR - len(data))


def image(path, partition=0):
    """A small disc: /default.xex, /data/big/db.big (two sectors), an empty /data/empty folder."""
    xex, big = b'XEX2' + bytes(range(200)), bytes(i % 251 for i in range(3000))
    sectors = {
        32: MAGIC + struct.pack('<II', 33, SECTOR) + b'\0' * (SECTOR - len(MAGIC) - 8),
        33: directory([('default.xex', 40, len(xex), 0x80), ('data', 34, SECTOR, DIRECTORY)]),
        34: directory([('big', 35, SECTOR, DIRECTORY), ('empty', 0, 0, DIRECTORY)]),
        35: directory([('db.big', 41, len(big), 0x80)]),
        40: xex + b'\0' * (SECTOR - len(xex)),
        41: big + b'\0' * (2 * SECTOR - len(big)),
    }
    with open(path, 'wb') as out:
        for sector, data in sectors.items():
            out.seek(partition + sector * SECTOR)
            out.write(data)
    return xex, big


class XisoTest(unittest.TestCase):
    def test_reads_named_files_from_each_partition_layout(self):
        for partition in (0, 0x2080000):
            with tempfile.TemporaryDirectory() as temp:
                disc = Path(temp) / 'disc.iso'
                xex, big = image(disc, partition)
                reader = Xiso(disc)
                self.assertEqual(reader.base, partition)
                out = Path(temp) / 'out'
                written = reader.extract(['default.xex', 'DATA/Big/DB.BIG', 'data/missing.big'], out)
                self.assertEqual(written, ['default.xex', 'DATA/Big/DB.BIG'])
                self.assertEqual((out / 'default.xex').read_bytes(), xex)
                self.assertEqual((out / 'DATA/Big/DB.BIG').read_bytes(), big)

    def test_folders_and_missing_paths_are_not_files(self):
        with tempfile.TemporaryDirectory() as temp:
            disc = Path(temp) / 'disc.iso'
            image(disc)
            reader = Xiso(disc)
            self.assertIsNone(reader.locate('data'))
            self.assertIsNone(reader.locate('data/big/db.big/x'))
            self.assertIsNone(reader.locate('data/empty/anything'))
            self.assertIsNone(reader.locate('nothing.bin'))

    def test_a_file_that_is_not_a_disc_image_is_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'not.iso'
            path.write_bytes(b'\0' * (40 * SECTOR))
            with self.assertRaises(ValueError):
                Xiso(path)


if __name__ == '__main__':
    unittest.main()
