import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from tools.asset_pipeline import install as core


class ToolDependency(unittest.TestCase):
    def test_non_windows_uses_path_tool_without_downloading(self):
        with tempfile.TemporaryDirectory() as temp:
            tool=Path(temp)/'extract-xiso'; tool.write_text('#!/bin/sh\n'); tool.chmod(0o755)
            with patch.dict(os.environ,{'PATH':temp}),\
                 patch.object(core,'download',side_effect=AssertionError('downloaded')):
                found=core.dependency(Path(temp)/'cache','extract-xiso',core.XISO_URL,core.XISO_SHA,lambda _:None,windows=False)
            self.assertEqual(found,tool)

    def test_non_windows_missing_tool_names_the_tool(self):
        with tempfile.TemporaryDirectory() as temp:
            with patch.dict(os.environ,{'PATH':temp}),\
                 patch.object(core,'download',side_effect=AssertionError('downloaded')):
                with self.assertRaisesRegex(RuntimeError,'extract-xiso is not on PATH'):
                    core.dependency(Path(temp)/'cache','extract-xiso',core.XISO_URL,core.XISO_SHA,lambda _:None,windows=False)

    def test_windows_uses_pinned_download_even_when_path_has_tool(self):
        with tempfile.TemporaryDirectory() as temp:
            cache=Path(temp)/'cache'
            def unpack(_archive,folder):
                (folder/'bin').mkdir(parents=True); (folder/'bin/extract-xiso.exe').write_bytes(b'')
            with patch.object(core.shutil,'which',return_value='/usr/bin/extract-xiso'),\
                 patch.object(core,'download',return_value=Path(temp)/'tool.zip') as download,\
                 patch.object(core,'unpack_zip',side_effect=unpack):
                found=core.dependency(cache,'extract-xiso',core.XISO_URL,core.XISO_SHA,lambda _:None,windows=True)
            download.assert_called_once()
            self.assertEqual(found,cache/'extract-xiso/bin/extract-xiso.exe')


if __name__=='__main__':unittest.main()
