import json
from pathlib import Path
import tempfile
import unittest

from tools.asset_pipeline.runtime_content import validate


class RuntimeContentTests(unittest.TestCase):
    def test_missing_hud_cannot_publish_complete_content(self):
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            assets = stage/'assets'
            def write(name, value=b'fixture'):
                path = assets/name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(value)
            write('private/game.json', json.dumps(dict(character_scene='private/skater.glb',
                  action_graph='private/action', motion_graph='private/motion')).encode())
            for name in ('private/skater.glb', 'private/action', 'private/motion',
                         'private/stock/data/anim/OnBoard.abin', 'private/stock/data/anim/OffBoard.abin'):
                write(name)
            for name in ('skater-collections.json', 'physics-skeletons.json', 'data/config/input.cfg',
                         'data/script/camera/Default_cameragraph.stategraph', 'data/camera/1.shk', 'data/camera/2.shk'):
                write('private/stock/'+name)
            with self.assertRaises(FileNotFoundError):
                validate(stage, [])
            self.assertFalse((assets/'private/content-manifest.json').exists())


if __name__ == '__main__':
    unittest.main()
