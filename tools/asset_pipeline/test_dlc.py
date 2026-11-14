"""The DLC package reader and the DLC input helpers: synthetic packages always, the player's own package when present."""
import os
from pathlib import Path
import struct
import tempfile
import unittest

from tools.asset_pipeline import dlc
from tools.asset_pipeline.stfs import BLOCK, StfsError, StfsPackage, find_packages

HEADER = 0xAD0E


def _offset(block):
    # The read-only LIVE layout (block shift 0): one level-0 hash table before every 170 data blocks, one level-1
    # table once the package passes 170 blocks.
    physical = block + (block + 170) // 170 + ((block + 28900) // 28900 if block >= 170 else 0)
    return 0xB000 + physical * BLOCK


def _package(path, files, title=0x454108E6, content=0x2, name='Test Pack'):
    """A LIVE package: a file table in block 0, then each (name, data, parent) in consecutive blocks."""
    head = bytearray(0xB000)
    head[:4] = b'LIVE'
    struct.pack_into('>III', head, 0x340, HEADER, content, 0)
    struct.pack_into('>I', head, 0x360, title)
    head[0x379] = 0x24
    head[0x37B] = 1                       # block separation
    head[0x37C:0x37E] = (1).to_bytes(2, 'little')   # file table: 1 block
    head[0x37E:0x381] = (0).to_bytes(3, 'little')   # at block 0
    head[0x411:0x411 + 2 * len(name)] = name.encode('utf-16-be')
    table = bytearray(BLOCK)
    blocks = {}
    next_block = 1
    for i, (file_name, data, parent, directory) in enumerate(files):
        count = 0 if directory else (len(data) + BLOCK - 1) // BLOCK
        row = bytearray(0x40)
        row[:len(file_name)] = file_name.encode()
        row[0x28] = len(file_name) | (0x80 if directory else 0x40)
        row[0x29:0x2C] = count.to_bytes(3, 'little')
        row[0x2F:0x32] = (0 if directory else next_block).to_bytes(3, 'little')
        row[0x32:0x34] = parent.to_bytes(2, 'big', signed=True)
        row[0x34:0x38] = (0 if directory else len(data)).to_bytes(4, 'big')
        table[i * 0x40:(i + 1) * 0x40] = row
        for b in range(count):
            blocks[next_block + b] = data[b * BLOCK:(b + 1) * BLOCK]
        next_block += count
    blocks[0] = bytes(table)
    with open(path, 'wb') as out:
        out.write(head)
        for block, data in sorted(blocks.items()):
            out.seek(_offset(block))
            out.write(data.ljust(BLOCK, b'\0'))
    return path


class StfsReader(unittest.TestCase):
    def test_reads_the_header_table_and_a_file_across_a_hash_table(self):
        payload = bytes(range(256)) * (BLOCK * 200 // 256) + b'tail'   # 200+ blocks: crosses the first hash table
        with tempfile.TemporaryDirectory() as temp:
            path = _package(Path(temp) / 'pack', [('data', b'', -1, True), ('pack.big', payload, 0, False)])
            package = StfsPackage(path)
            self.assertEqual((package.magic, package.display_name), ('LIVE', 'Test Pack'))
            self.assertTrue(package.is_skate3_dlc())
            entry = package.files()[0]
            self.assertEqual((entry.path, entry.size), ('data/pack.big', len(payload)))
            out = package.extract(entry, Path(temp) / 'out/pack.big')
            self.assertEqual(out.read_bytes(), payload)
            # A range straddling data block 169 / 170 (a hash table between them on disk).
            start = 168 * BLOCK + 100
            self.assertEqual(package.read(entry, start, 3 * BLOCK), payload[start:start + 3 * BLOCK])

    def test_other_titles_and_chained_files_are_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            _package(Path(temp) / 'other', [('x.big', b'x' * 10, -1, False)], title=0x12345678)
            self.assertEqual(find_packages(Path(temp)), [])
            path = _package(Path(temp) / 'pack', [('x.big', b'x' * (BLOCK * 2), -1, False)])
            package = StfsPackage(path)
            chained = package.files()[0].__class__(**{**package.files()[0].__dict__, 'consecutive': False})
            with self.assertRaises(StfsError):
                package.read(chained, 0, 4)


class DlcInput(unittest.TestCase):
    def test_no_folder_means_no_identity_and_the_recomp_layout_is_found(self):
        self.assertEqual(dlc.signature(None), '')
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'game').mkdir()
            (root / 'dlc').mkdir()
            env = os.environ.pop(dlc.ENV, None)
            try:
                self.assertEqual(dlc.folder(root / 'game/default.xex'), (root / 'dlc').resolve())
                self.assertEqual(dlc.signature(root / 'dlc'), '')
            finally:
                if env is not None:
                    os.environ[dlc.ENV] = env

    def test_a_dlc_folder_next_to_the_game_copy_and_its_listing(self):
        # The release ZIP: no variable, nothing saved, a `dlc` folder next to skate3rust.exe (the laptop import,
        # 2026-10-21). The listing is what the game's start-up check compares (asset_check.rs, same format).
        with tempfile.TemporaryDirectory() as temp:
            game = Path(temp) / 'game copy'
            (game / 'dlc/Pack/454108E6').mkdir(parents=True)
            (game / 'dlc/Pack/454108E6/PKG').write_bytes(b'1234567')
            (game / 'dlc/TU_file').write_bytes(b'123')
            env = os.environ.pop(dlc.ENV, None)
            try:
                self.assertIsNone(dlc.folder(None))
                self.assertEqual(dlc.folder(None, None, game), (game / 'dlc').resolve())
                self.assertEqual(dlc.listing(game / 'dlc'), ['Pack/454108E6/PKG|7', 'TU_file|3'])
                self.assertEqual(dlc.listing(None), [])
            finally:
                if env is not None:
                    os.environ[dlc.ENV] = env

    def test_a_district_needs_its_sim_index_and_sim_cells(self):
        s = 'data/content/world/stream/'
        paths = [s + 'DIST_MaloofMoneyCupDLC/DIST_MaloofMoneyCupDLC_Sim.xst', s + 'DIST_MaloofMoneyCupDLC/cSim_0_0_high.xsf',
                 # A proxy with an index but no cells, and loose files outside a stream folder, are not maps.
                 s + 'DIST_AGParkProxy/DIST_AGParkProxy_Sim.xst', s + 'DIST_AGParkProxy/x.loc',
                 'data/audio/DLC_Maloof.ems']
        self.assertEqual(dlc.stream_districts(paths), ['DIST_MaloofMoneyCupDLC'])
        self.assertTrue(dlc.has_maps(paths))
        self.assertFalse(dlc.has_maps(paths[2:]))

    def test_language_tables_with_one_nul_more_or_less_pair_up(self):
        def table(strings, trailing):
            pool = b'\0'.join(s.encode() for s in strings) + trailing
            return struct.pack('<IIIII', 0, 0, len(strings) - 1, 28, 28) + b'\0' * 8 + b'\0' * 8 + pool
        # Declared count n holds n + 2 strings with the disc's double NUL ending.
        disc = dlc._strings(table(['', 'A', 'B'], b'\0'))
        short = dlc._strings(table(['', 'A', 'B'], b''))
        long = dlc._strings(table(['', 'A', 'B'], b'\0\0'))
        self.assertEqual(disc, short)
        self.assertEqual(disc, long)


@unittest.skipUnless(Path(os.environ.get('SKATE3_TEST_CREATE_PACK', r'I:\QuiverLauncher-win-Portable\Apps'
                                         r'\Skate3-Skate3Recompiled\dlc\Create Park Upgrade')).is_dir(),
                     "the player's Create Park Upgrade package is not here")
class OwnedCreatePack(unittest.TestCase):
    def test_the_create_pack_carries_the_dlc_replay_editor(self):
        folder = Path(os.environ.get('SKATE3_TEST_CREATE_PACK', r'I:\QuiverLauncher-win-Portable\Apps'
                                     r'\Skate3-Skate3Recompiled\dlc\Create Park Upgrade'))
        package = find_packages(folder)[0]
        self.assertEqual(package.display_name, 'Skate.Create Upgrade Pack')
        big = next(e for e in package.files() if e.path.endswith('.big'))
        paths = dlc.big_paths(package, big)
        self.assertEqual(len(paths), 1262)
        self.assertIn('data/fe/source/screens/replay2/replay_editor3.apt', [p.lower() for p in paths])


if __name__ == '__main__':
    unittest.main()
