"""Installation selection and refresh checks using synthetic files only."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from tools.asset_pipeline import versions as v
from tools.asset_pipeline.install import install


class AssetVersions(unittest.TestCase):
    def test_hud_change_does_not_reconvert_maps(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            (root/'prepare_hud.py').write_text('before')
            before=v.fingerprints(root)
            (root/'prepare_hud.py').write_text('after')
            self.assertEqual(v.changed_groups(before,v.fingerprints(root)), {'hud'})
            (root/'asset_pipeline').mkdir()
            (root/'asset_pipeline/vlt.py').write_text('new decoder')
            self.assertEqual(v.changed_groups(before,v.fingerprints(root)),set(v.GROUPS))

    def test_fresh_copy_never_adopts_other_installation(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            (root/'installation.json').write_text(json.dumps({'version':1,'directory':'../old'}))
            with self.assertRaises(ValueError):v.installed(root)
            self.assertIsNone(v.installed(root/'new-copy/data'))

    def fixture(self, root):
        base=root/'copy/data';old=base/('installations/'+'a'*32)
        private=old/'assets/private';(private/'stock').mkdir(parents=True)
        (private/'game.json').write_text('{}');(private/'stock/skater-collections.json').write_text('{}')
        (private/'hud').mkdir();(private/'hud/old.txt').write_text('old')
        (old/'maps').mkdir();(old/'maps/University.skate').write_bytes(b'unchanged map')
        (old/'settings').mkdir();(old/'settings/default-map.json').write_text('"custom.skate"')
        (old/'maps.json').write_text('[{"name":"University"}]')
        current={group:'new' for group in v.GROUPS}
        marker={'version':1,'directory':'installations/'+'a'*32,'pipelines':{**current,'hud':'old'}}
        (base/'installation.json').write_text(json.dumps(marker))
        source=root/'disc'
        for name in ('default.xex','data/big/miscload.big','data/big/miscboot.big','data/big/db.big',
                     'data/content/createacharacter.big','data/content/worldDIST_University.big'):
            p=source/name;p.parent.mkdir(parents=True,exist_ok=True);p.touch()
        return base,old,source,current,marker

    def test_refresh_preserves_maps_and_publishes_only_after_validation(self):
        with tempfile.TemporaryDirectory() as temp:
            base,old,source,current,marker=self.fixture(Path(temp))
            calls=[]
            def run(args,*unused):
                calls.append([str(a) for a in args])
            with patch.object(v,'fingerprints',return_value=current), patch('tools.asset_pipeline.install.run',side_effect=run):
                new=install(source,base,Path('unused.exe'),lambda _:None,refresh=True)
            self.assertEqual((new/'maps/University.skate').read_bytes(),b'unchanged map')
            self.assertEqual((new/'settings/default-map.json').read_text(),'"custom.skate"')
            self.assertTrue((old/'assets/private/hud/old.txt').is_file())
            self.assertFalse((new/'assets/private/hud/old.txt').exists())
            self.assertEqual(v.installed(base)[1]['pipelines'],current)
            self.assertTrue(any(any('prepare_runtime_huds.py' in a for a in args) for args in calls))
            self.assertFalse(any(any('map_job.py' in a for a in args) for args in calls))

    def test_failed_refresh_keeps_previous_record(self):
        with tempfile.TemporaryDirectory() as temp:
            base,old,source,current,marker=self.fixture(Path(temp))
            with patch.object(v,'fingerprints',return_value=current), patch('tools.asset_pipeline.install.run',side_effect=RuntimeError('failed')):
                with self.assertRaises(RuntimeError):install(source,base,Path('unused.exe'),lambda _:None,refresh=True)
            self.assertEqual(v.installed(base)[1],marker)
            self.assertFalse((base/'setup.lock').exists())


if __name__=='__main__':unittest.main()
