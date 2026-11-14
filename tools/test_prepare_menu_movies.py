"""prepare_menu_movies: install/verify rules (always) and the real export (data-gated on the owned disc).

Data-gated parts look for the extracted disc and the installed skater collections in
SKATE3_DISC / SKATE3_COLLECTIONS, else in the MAIN checkout (`.local/skate3-disc`,
`assets/private/stock/skater-collections.json`), found through git's common dir so the
test also finds them from a worktree. Output goes to a temporary folder only.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

# The repo root, as the production entry has it (prepare_runtime_huds.py): the DLC readers import `tools.owned_game`.
sys.path.insert(1, str(Path(__file__).resolve().parents[1]))

import prepare_menu_movies as menus  # noqa: E402


def main_checkout():
    here = Path(__file__).resolve().parent
    try:
        common = subprocess.run(['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'], cwd=here,
                                capture_output=True, text=True, check=True).stdout.strip()
        return Path(common).parent
    except (OSError, subprocess.CalledProcessError):
        return here.parent


class MenuMovieRules(unittest.TestCase):
    def test_library_key_matches_the_engine(self):
        self.assertEqual(menus.library_key('data\\fe\\source\\controls\\panel.apt'), 'source/controls/panel')
        self.assertEqual(menus.library_key('source/controls/panel'), 'source/controls/panel')

    def test_named_images_copy_single_texture_bundles_under_the_prefix(self):
        with tempfile.TemporaryDirectory() as folder:
            cache, runtime = Path(folder) / 'cache', Path(folder) / 'runtime'

            def bundle(name, textures):
                manifest = f'assets/{name}/manifest.json'
                (cache / manifest).parent.mkdir(parents=True, exist_ok=True)
                (cache / manifest).write_text(json.dumps({'textures': textures}), encoding='utf-8')
                for t in textures:
                    (cache / f'assets/{name}' / t['rgba_file']).write_bytes(bytes(16))
                return {'name': name, 'textures': {'manifest': manifest}}

            one = [{'rgba_file': '0000_test.Texture.rgba', 'width': 2, 'height': 2}]
            bundles = [bundle('data/fe/source/images/map/test', one), bundle('data/fe/source/other/x', one)]
            cache.mkdir(parents=True, exist_ok=True)
            (cache / 'manifest.json').write_text(json.dumps({'bundles': bundles}), encoding='utf-8')
            images = menus.named_images(cache, runtime, ('data/fe/source/images/map/',))
            self.assertEqual(list(images), ['fe/source/images/map/test'])
            image = images['fe/source/images/map/test']
            self.assertEqual((image['width'], image['height']), (2, 2))
            self.assertEqual((runtime / image['rgba']).read_bytes(), bytes(16))
            two = one + [{'rgba_file': '0001_b.Texture.rgba', 'width': 2, 'height': 2}]
            bundles.append(bundle('data/fe/source/images/map/two', two))
            (cache / 'manifest.json').write_text(json.dumps({'bundles': bundles}), encoding='utf-8')
            with self.assertRaises(ValueError):
                menus.named_images(cache, runtime, ('data/fe/source/images/map/',))

    def test_install_verifies_hashes_and_replaces(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            runtime, assets = root / 'runtime', root / 'assets'
            (runtime / 'movies').mkdir(parents=True)
            (runtime / 'movies/a.json').write_text('{}')
            files = {'movies/a.json': menus._digest(runtime / 'movies/a.json')}
            manifest = {'format': menus.FORMAT, 'version': menus.VERSION, 'movies': {'a': 'movies/a.json'},
                        'files': files}
            (runtime / 'manifest.json').write_text(json.dumps(manifest))
            (assets / 'private/menu-movies').mkdir(parents=True)
            (assets / 'private/menu-movies/stale.json').write_text('old')
            installed = menus.install(assets, runtime)
            self.assertIn('private/menu-movies/movies/a.json', installed)
            self.assertFalse((assets / 'private/menu-movies/stale.json').exists())
            (runtime / 'movies/a.json').write_text('{"changed":1}')
            with self.assertRaises(ValueError):
                menus.install(assets, runtime)
            # A failed install leaves the previous set in place.
            self.assertTrue((assets / 'private/menu-movies/manifest.json').is_file())

    def test_clip_action_table_layout_and_bounds(self):
        # count 2 at 0, events at 8: {flags, key_code, actions} x 2 (big-endian).
        words = [2, 8, 0x40000, 0, 100, 0x1, 0, 200]
        apt = b''.join(w.to_bytes(4, 'big') for w in words)
        self.assertEqual(menus.clip_actions(apt, 0), [
            {'flags': 0x40000, 'key_code': 0, 'actions_offset': 100},
            {'flags': 0x1, 'key_code': 0, 'actions_offset': 200}])
        with self.assertRaises(ValueError):
            menus.clip_actions(apt[:20], 0)  # second event outside the movie
        with self.assertRaises(ValueError):
            menus.clip_actions((65).to_bytes(4, 'big') + bytes(4), 0)  # over the cap

    def test_textured_units_get_their_bitmap_id(self):
        bundle = {'shapes': {3: {'units': [{'texture_id': 0}, {'texture_id': 7}]}}}
        textured = menus._with_bitmap(bundle, {'character_id': 3, 'unit': 1, 'texture': {'rgba': 'x'}})
        self.assertEqual(textured['bitmap'], 7)
        solid = menus._with_bitmap(bundle, {'character_id': 3, 'unit': 0, 'texture': None})
        self.assertNotIn('bitmap', solid)

    def test_paths_cannot_escape(self):
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaises(ValueError):
                menus._contained(Path(temp), '../x')


class RetailMenuExport(unittest.TestCase):
    def test_exports_every_menu_movie_and_import(self):
        main = main_checkout()
        disc = Path(os.environ.get('SKATE3_DISC', main / '.local/skate3-disc'))
        collections = Path(os.environ.get('SKATE3_COLLECTIONS',
                                          main / 'assets/private/stock/skater-collections.json'))
        if not (disc / 'data/big/fedata.big').is_file() or not collections.is_file():
            self.skipTest(f'no owned disc / collections at {disc}, {collections}')
        with tempfile.TemporaryDirectory() as temp:
            manifest = menus.prepare(disc, Path(temp), collections)
            runtime = Path(temp) / 'runtime'
            self.assertEqual(manifest['missing_movies'], [])
            self.assertEqual(manifest['unresolved_fonts'], [])
            for root in menus.MENU_ROOTS:
                self.assertIn(root, manifest['movies'])
            exported = {}
            for key, file in manifest['movies'].items():
                movie = json.loads((runtime / file).read_text(encoding='utf-8'))
                self.assertEqual(movie['movie'], key)
                self.assertTrue(movie['characters'])
                exported[key] = movie
            # Every import source is part of the set, and the named export exists there.
            for key, movie in exported.items():
                for record in movie['imports']:
                    source = exported[menus.library_key(record['file'])]
                    self.assertIn(record['name'], [e['name'] for e in source['exports']], key)
            # Milestone 4: every textured unit names a bitmap character of its own movie whose
            # texture id is the payload it was resolved through; shapes all resolve.
            textured = 0
            for key, movie in exported.items():
                self.assertEqual(movie['unresolved_shapes'], {}, key)
                chars = {c['id']: c for c in movie['characters']}
                for units in movie['shapes'].values():
                    for unit in units:
                        if not unit.get('texture'):
                            self.assertNotIn('bitmap', unit)
                            continue
                        textured += 1
                        bitmap = chars[unit['bitmap']]
                        self.assertEqual(bitmap['type_name'], 'bitmap')
                        stem = unit['texture']['resource_name'].replace(chr(92), '/').rsplit('/', 1)[-1]
                        self.assertEqual(stem.split('.')[0], str(bitmap['bitmap']['texture_id']))
            self.assertGreater(textured, 0)
            files = menus.runtime_files(runtime)
            self.assertEqual(len(files), len(manifest['files']) + 1)


OWNED_MAP_PACK = Path(os.environ.get('SKATE3_TEST_MAP_PACK', r'F:\Libraries\Downloads\Xbox 360 Skate 3 DLCs (World)'
                                     r'\Skate 3 DLCs\Maloof Money Cup 2010 NYC'))


@unittest.skipUnless(OWNED_MAP_PACK.is_dir(), "the player's Maloof Money Cup 2010 NYC package is not here")
class OwnedMapPackText(unittest.TestCase):
    def test_a_map_pack_names_its_location_in_the_menu_text(self):
        # Laptop session 2026-10-21: the Challenge Map showed ID_WORLD_MALOOFMONEYCUP_NYC; a map pack carries no menu
        # movie, so its text tables were never read into the menus' language.
        from asset_pipeline import dlc as packages
        with tempfile.TemporaryDirectory() as work:
            text = {}
            for archive in menus.dlc_text_archives(OWNED_MAP_PACK, Path(work)):
                text.update(packages.language(archive))
            self.assertEqual(text.get('ID_WORLD_MALOOFMONEYCUP_NYC'), 'Maloof Money Cup 2010 NYC')


if __name__ == '__main__':
    unittest.main()
