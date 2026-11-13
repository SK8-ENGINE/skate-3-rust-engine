"""menu_tables.convert through the real setup spawn(), with a stand-in game exe."""
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from . import install, menu_tables

# Mimics `skate3rust --extract-menu-tables <xex> <out>` (made-up ids only).
FAKE_GAME = '''
import json, sys
mode, xex, out = sys.argv[1:4]
assert mode == '--extract-menu-tables'
if open(xex, 'rb').read(4) != b'XEX2':
    print('Menu table extraction failed: Not an XEX2 executable', file=sys.stderr)
    sys.exit(1)
item = {'name': 'A', 'label': 'B', 'icon': 'c', 'sub_option_kind': -1, 'help': 'd'}
open(out, 'w').write(json.dumps({'schema': 1, 'items': [item, item], 'modes': [{'key': 'M'}], 'templates': [{'name': 'CrossBar', 'path': 'main/core_menu.swf', 'kind': 0}], 'screens': [{'key': '0000000000000000', 'blur': True}], 'settings_titles': ['ID_GAMESETTINGS_TITLE'], 'sub_options': [{'labels': ['ID_A'], 'helpers': ['']}], 'trick_guide': {'categories': ['046ed0ccbda1091f'], 'gestures': [''], 'buttons': ['']}, 'career_stats': [{'label': 'ID_RECORD_A', 'enabled': True}]}))
print('MENU_TABLES_READY')
'''


class ConvertTests(unittest.TestCase):
    def run_convert(self, xex_bytes):
        root = Path(self.temporary.name)
        script = root / 'fake_game.py'
        script.write_text(FAKE_GAME)
        xex = root / 'default.xex'
        xex.write_bytes(xex_bytes)
        assets = root / 'assets'
        real_spawn = install.spawn
        with mock.patch.object(install, 'spawn',
                               lambda args, **kw: real_spawn([sys.executable, script, *args[1:]], **kw)):
            return menu_tables.convert(root / 'skate3rust.exe', xex, assets), assets

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.temporary.cleanup()

    def test_writes_the_tables_into_private_assets(self):
        count, assets = self.run_convert(b'XEX2' + bytes(16))
        self.assertEqual(count, 2)
        self.assertEqual(json.loads((assets / 'private/menu-tables.json').read_text())['schema'], 1)

    def test_failure_leaves_no_file(self):
        with self.assertRaises(RuntimeError) as raised:
            self.run_convert(b'NOPE')
        self.assertIn('Not an XEX2 executable', str(raised.exception))
        self.assertFalse((Path(self.temporary.name) / 'assets/private/menu-tables.json').exists())


if __name__ == '__main__':
    unittest.main()
