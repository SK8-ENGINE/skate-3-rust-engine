import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from . import native_roster
from .customisation_library import requested_textures
from .customisation_workers import customiser_workers


def fake_roster(keys):
    return [{'key': k, 'recipe': k, 'name': k.title(), 'category': 'Pro', 'animation_style': 'Aggressive'} for k in keys]


class RosterParallelTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.collections = self.root / 'collections.json'
        self.collections.write_text(json.dumps({'collections': []}))
        self.work = self.root / 'work'

    def tearDown(self):
        self.temp.cleanup()

    def call(self, keys, workers, run_parallel):
        with patch.object(native_roster, 'roster', return_value=fake_roster(keys)), \
             patch('tools.asset_pipeline.customisation_workers.run_parallel', side_effect=run_parallel), \
             patch.object(native_roster, 'prepare', side_effect=lambda *a, **k: [{'key': 'serial'}]) as serial:
            result = native_roster.prepare_parallel(self.root, self.root, self.root / 'lib', self.collections, self.work, workers)
        return result, serial

    def test_reports_are_merged_back_into_roster_order(self):
        keys = ['a', 'b', 'c', 'd', 'e']
        seen = []
        def workers_finish_out_of_order(requests, workers):
            for _, request in reversed(requests):
                data = json.loads(Path(request).read_text())
                seen.append(data['only'])
                Path(data['report']).write_text(json.dumps([{'key': k, 'status': 'ready'} for k in reversed(data['only'])]))
            return [None] * len(requests)
        result, serial = self.call(keys, 2, workers_finish_out_of_order)
        self.assertEqual([r['key'] for r in result], keys)
        self.assertEqual(sorted(k for chunk in seen for k in chunk), keys)
        self.assertTrue(all(seen))  # no worker got an empty (= "all") key list
        serial.assert_not_called()
        self.assertEqual(json.loads((self.work / 'report.json').read_text()), result)

    def test_a_failed_worker_falls_back_to_serial_prepare(self):
        result, serial = self.call(['a', 'b', 'c'], 3, lambda requests, workers: [RuntimeError('boom')] + [None] * (len(requests) - 1))
        self.assertEqual(result, [{'key': 'serial'}])
        serial.assert_called_once()

    def test_workers_never_exceed_characters(self):
        requested = []
        def record(requests, workers):
            requested.append(len(requests))
            for _, request in requests:
                data = json.loads(Path(request).read_text())
                Path(data['report']).write_text(json.dumps([{'key': k} for k in data['only']]))
            return [None] * len(requests)
        self.call(['a', 'b'], 8, record)
        self.assertEqual(requested, [2])
        self.assertGreaterEqual(customiser_workers(), 1)
        self.assertLessEqual(customiser_workers(), 8)

    def test_publish_is_atomic_and_leaves_no_temporary_files(self):
        dest = self.root / 'decoded' / 'abc.png'
        dest.parent.mkdir()
        native_roster._publish(dest, lambda tmp: tmp.write_bytes(b'png'))
        self.assertEqual(dest.read_bytes(), b'png')
        with self.assertRaises(ValueError):
            native_roster._publish(self.root / 'decoded/bad.png', lambda tmp: (tmp.write_bytes(b'x'), (_ for _ in ()).throw(ValueError()))[1])
        self.assertEqual(sorted(p.name for p in dest.parent.iterdir()), ['abc.png'])

    def test_publish_tolerates_windows_rename_contention(self):
        # WinError 5: another worker published dest first or is reading it.
        dest = self.root / 'decoded' / 'shared.png'
        dest.parent.mkdir()
        dest.write_bytes(b'png')
        # Another worker published between our write and rename: keep theirs,
        # never rename over a file a reader may be opening.
        published = []
        native_roster._publish(dest, lambda tmp: (tmp.write_bytes(b'png'), published.append(1)))
        self.assertEqual(published, [1])
        self.assertEqual(sorted(p.name for p in dest.parent.iterdir()), ['shared.png'])
        mover = 'rename' if os.name == 'nt' else 'replace'
        with patch.object(native_roster.os, mover, side_effect=FileExistsError(17, 'exists')):
            native_roster._publish(self.root / 'decoded/raced.png', lambda tmp: tmp.write_bytes(b'png'))
        self.assertEqual(sorted(p.name for p in dest.parent.iterdir()), ['shared.png'])
        # Transient contention before dest exists is retried, not fatal.
        fresh = self.root / 'decoded' / 'fresh.png'
        real, calls = getattr(os, mover), []
        def flaky(a, b):
            calls.append(1)
            if len(calls) < 3:
                raise PermissionError(5, 'Access is denied')
            real(a, b)
        with patch.object(native_roster.os, mover, side_effect=flaky), patch.object(native_roster.time, 'sleep'):
            native_roster._publish(fresh, lambda tmp: tmp.write_bytes(b'new'))
        self.assertEqual(fresh.read_bytes(), b'new')
        self.assertEqual(len(calls), 3)


class WarmTextureTests(unittest.TestCase):
    def test_requested_textures_follow_prepare_skip_rules(self):
        def mat(*channels):
            return {'textures': [{'channel': c, 'id': f'{c}{len(channels)}'} for c in channels], 'flags': {}}
        catalog = {'materials': {
            'm1': mat('diffuse', 'normal'), 'm2': mat('normal'),          # m2: no diffuse -> skipped
            'm3': mat('diffuse', 'alpha', 'specular'),
            't1': {'textures': [{'channel': 'decal', 'id': 'tat'}], 'flags': {'cas.TattooCategory': 'arm'}}},
            'components': [
                {'slot': 'Hat', 'models': [{'lods': [{'index': 0, 'material_instances': [[{'id': 'm1'}, {'id': 'm2'}]]}]},
                                           {'lods': [{'index': 1, 'material_instances': [[{'id': 'm3'}]]}]},   # no LOD 0
                                           {'lods': [{'index': 0, 'material_instances': [[{'id': 'm1'}], [{'id': 'm3'}]]}]}]},
                {'slot': 'Misc', 'models': [{'lods': [{'index': 0, 'material_instances': [[{'id': 't1'}]]}]}]}]}
        self.assertEqual(requested_textures(catalog), ['diffuse2', 'normal2', 'diffuse3', 'alpha3', 'specular3', 'tat'])


if __name__ == '__main__':
    unittest.main()
