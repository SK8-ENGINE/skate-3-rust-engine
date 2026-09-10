"""Setup output placement checks; no game or retail data required."""
import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parent))
import prepare_runtime_huds as huds


class HudWorkspace(unittest.TestCase):
    def test_long_install_uses_short_temporary_workspace_and_keeps_outputs(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            assets=root/'assets'
            long_work=root/('nested-install-'*12)/'conversion/hud'
            seen=[]
            def prepare(game, output, *args):
                seen.append(output)
                self.assertNotIn(long_work,output.parents)
                self.assertLess(len(str(output/'source-cache/assets/data/fe/source/images/buttons/xbox360/buttons/0000_button_DPad_Down_hud.Texture.texture.bin')),260)
                output.mkdir(parents=True)
            def install(destination,scoring,marker):
                destination.mkdir()
                (destination/'verified').write_bytes(b'original artwork')
                return ['verified']
            with patch.object(huds,'prepare_scoring',side_effect=prepare),patch.object(huds,'prepare_marker',side_effect=prepare),patch.object(huds,'install',side_effect=install):
                self.assertEqual(huds.prepare(root/'game',assets,long_work),['verified'])
            self.assertTrue((assets/'verified').is_file())
            self.assertTrue(all(not p.exists() for p in seen))

    def test_extraction_failure_never_publishes(self):
        with patch.object(huds,'prepare_scoring',side_effect=RuntimeError('decode failed')),patch.object(huds,'install') as install:
            with self.assertRaisesRegex(RuntimeError,'decode failed'):
                huds.prepare(Path('game'),Path('assets'),Path('work'))
            install.assert_not_called()
