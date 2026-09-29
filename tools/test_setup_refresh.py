"""Setup helper refresh helpers that avoid forcing another ISO picker."""
import tempfile
import unittest
from pathlib import Path
from tools import setup as setup_ui


class SetupRefresh(unittest.TestCase):
    def test_saved_source_requires_an_existing_path(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            missing = root/'gone'/'default.xex'
            self.assertIsNone(setup_ui.saved_source({'source': str(missing)}))
            self.assertIsNone(setup_ui.saved_source({}))
            self.assertIsNone(setup_ui.saved_source({'source': 1}))
            disc = root/'game'
            disc.mkdir()
            xex = disc/'default.xex'
            xex.write_bytes(b'xex')
            self.assertEqual(setup_ui.saved_source({'source': str(xex)}), xex)
            self.assertEqual(setup_ui.saved_source({'source': str(disc)}), disc)


if __name__ == '__main__':
    unittest.main()
