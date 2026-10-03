"""ocean_pca.convert through the real setup spawn(), with a stand-in game exe."""
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from . import install, ocean_pca

# Mimics `skate3rust --extract-ocean-pca <xex> <out>`.
FAKE_GAME = '''
import json, sys
mode, xex, out = sys.argv[1:4]
assert mode == '--extract-ocean-pca'
if open(xex, 'rb').read(4) != b'XEX2':
    print('Ocean PCA extraction failed: Not an XEX2 executable', file=sys.stderr)
    sys.exit(1)
frames = [[[0.5, 0.5, 1.0, 0.0]] + [[0.0] * 4] * 6] * 30
open(out, 'w').write(json.dumps({'hz': 30.0, 'frames': frames}))
print('OCEAN_PCA_READY')
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
        # The game exe slot runs the fake through the same spawn() setup uses.
        with mock.patch.object(install, 'spawn',
                               lambda args, **kw: real_spawn([sys.executable, script, *args[1:]], **kw)):
            return ocean_pca.convert(root / 'skate3rust.exe', xex, assets), assets

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.temporary.cleanup()

    def test_writes_thirty_frames_into_private_assets(self):
        count, assets = self.run_convert(b'XEX2' + bytes(16))
        self.assertEqual(count, 30)
        data = json.loads((assets / 'private/ocean-pca.json').read_text())
        self.assertEqual(data['hz'], 30.0)

    def test_failure_is_optional_content_error_and_leaves_no_file(self):
        with self.assertRaises(RuntimeError) as raised:
            self.run_convert(b'NOPE')
        self.assertIn('Not an XEX2 executable', str(raised.exception))
        self.assertFalse((Path(self.temporary.name) / 'assets/private/ocean-pca.json').exists())


if __name__ == '__main__':
    unittest.main()
