"""Fresh/update character publication without a game process or retail data."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from tools.asset_pipeline import customiser_setup as s


class CharacterSetup(unittest.TestCase):
    def setUp(self):
        source = patch.object(s, 'source_fingerprint', return_value='owned-disc')
        source.start()
        self.addCleanup(source.stop)

    def test_failed_generation_preserves_previous_selection_and_assets(self):
        with tempfile.TemporaryDirectory() as temp:
            assets = Path(temp)/'assets'
            base = assets/'private/customisation'; base.mkdir(parents=True)
            old = {'set': 'a'*32, 'fingerprint': 'old'}
            (base/'current.json').write_text(json.dumps(old))
            with patch('tools.asset_pipeline.customisation_catalog.prepare', side_effect=RuntimeError('bad source')):
                with self.assertRaises(RuntimeError): s.prepare(Path(temp)/'source', assets, lambda _: None)
            self.assertEqual(json.loads((base/'current.json').read_text()), old)

    def test_fresh_generation_contains_customiser_profiles_lighting_and_native_roster(self):
        with tempfile.TemporaryDirectory() as temp:
            assets = Path(temp)/'assets'
            data = dict(models={'body': {}}, materials={'cloth': {}}, errors=[],
                        defaults={'male': {'selections': {'Body': {'asset_id': 'body', 'material_id': 'cloth'}}}})
            def catalog(game, out):
                (out/'native.json').write_text('{}')
            def library(config):
                (Path(config['directory'])/'library-v3.json').write_text(json.dumps(data))
                return data
            def lighting(game, assets, directory, data):
                (directory/'native-lighting.json').write_text('{"pro":{}}')
            def roster(game, assets, library, collections, work):
                library.mkdir(exist_ok=True)
                return roster_results.pop(0)
            roster_results = [[], [{'status': 'ready', 'key': 'pro'}]]
            with patch('tools.asset_pipeline.customisation_catalog.prepare', side_effect=catalog), \
                 patch('tools.asset_pipeline.customisation_library.prepare', side_effect=library), \
                 patch('tools.asset_pipeline.customisation_profiles.generate', return_value=[]), \
                 patch('tools.asset_pipeline.customiser_lighting.prepare', side_effect=lighting), \
                 patch('tools.asset_pipeline.native_roster.prepare', side_effect=roster):
                with self.assertRaisesRegex(RuntimeError, 'roster preparation'):
                    s.prepare(Path(temp)/'source', assets, lambda _: None)
                self.assertFalse((assets/'private/customisation/current.json').exists())
                s.prepare(Path(temp)/'source', assets, lambda _: None)
            base = assets/'private/customisation'
            current = json.loads((base/'current.json').read_text())
            generation = base/'sets'/current['set']
            for path in ('library-v3.json', 'extra-menu.json', 'native-lighting.json', 'native-roster/complete.json'):
                self.assertTrue((generation/path).is_file(), path)
            with patch('tools.asset_pipeline.customisation_catalog.prepare', side_effect=AssertionError('must reuse')):
                s.prepare(Path(temp)/'source', assets, lambda _: None)

    def test_character_only_update_reuses_core_installation(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); source = root/'source'; source.mkdir()
            base = root/'data'; installed = base/'installations/old'
            with patch('tools.asset_pipeline.install.install', return_value=installed) as install, \
                 patch.object(s, 'prepare') as prepare:
                s.install(source, base, Path('unused.exe'), lambda _: None, refresh=True)
            self.assertTrue(install.call_args.kwargs['refresh'])
            self.assertEqual(install.call_args.kwargs['game_root'], source)
            prepare.assert_called_once()
            self.assertEqual(prepare.call_args.args[:2], (source, installed/'assets'))


if __name__ == '__main__': unittest.main()
