import json
import os
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch

from . import native_roster
from .customisation_library import derived_jobs, requested_textures, warm_up
from .customisation_workers import batches, claimed, customiser_workers, run_shared
from .setup_budget import cores, customiser_limit, load_budget


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

    def call(self, keys, workers, run_shared):
        with patch.object(native_roster, 'roster', return_value=fake_roster(keys)),              patch('tools.asset_pipeline.customisation_workers.run_shared', side_effect=run_shared),              patch.object(native_roster, 'prepare', side_effect=lambda *a, **k: [{'key': 'serial'}]) as serial:
            result = native_roster.prepare_parallel(self.root, self.root, self.root / 'lib', self.collections, self.work, workers)
        return result, serial

    def test_reports_are_merged_back_into_roster_order(self):
        keys = ['a', 'b', 'c', 'd', 'e']
        seen = []
        def batches_finish_out_of_order(payload, work, workers):
            for index in reversed(range(len(work))):
                seen.append(work[index]['items'])
                Path(payload['reports'], f'{index}.json').write_text(json.dumps([{'key': k, 'status': 'ready'} for k in work[index]['items']]))
            return [], set(range(len(work)))
        result, serial = self.call(keys, 2, batches_finish_out_of_order)
        self.assertEqual([r['key'] for r in result], keys)
        self.assertEqual(sorted(k for chunk in seen for k in chunk), keys)
        self.assertTrue(all(seen))  # no batch has an empty (= "all") key list
        serial.assert_not_called()
        self.assertEqual(json.loads((self.work / 'report.json').read_text()), result)

    def test_a_failed_worker_falls_back_to_serial_prepare(self):
        result, serial = self.call(['a', 'b', 'c'], 3, lambda payload, work, workers: ([RuntimeError('boom')], set()))
        self.assertEqual(result, [{'key': 'serial'}])
        serial.assert_called_once()

    def test_one_character_is_prepared_serially(self):
        result, serial = self.call(['a'], 8, lambda *a: self.fail('no pool for one character'))
        self.assertEqual(result, [{'key': 'serial'}])
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


class SharedPoolTests(unittest.TestCase):
    """run_shared with the worker processes replaced by threads running the same claim loop."""
    def run_pool(self, items, workers, limit):
        done, active, peak = [], [0], [0]
        lock = threading.Lock()
        def fake_run(args, log, report):
            request = json.loads(Path(args[-1]).read_text())
            with lock:
                active[0] += 1;peak[0] = max(peak[0], active[0])
            try:
                for index in claimed(request):
                    time.sleep(0.01)
                    with lock:done.extend(request['batches'][index]['items'])
            finally:
                with lock:active[0] -= 1
        with patch('tools.asset_pipeline.install.run', side_effect=fake_run),              patch('tools.asset_pipeline.install.task', side_effect=lambda *a: list(a)),              patch('tools.asset_pipeline.setup_budget.customiser_limit', side_effect=limit):
            errors, taken = run_shared({}, batches('x', items, 2), workers, poll=0.005)
        return done, peak[0], errors, taken

    def test_every_batch_runs_exactly_once(self):
        items = list(range(41))
        done, peak, errors, taken = self.run_pool(items, 3, lambda workers, running: workers)
        self.assertEqual(sorted(done), items)
        self.assertEqual(errors, [])
        self.assertEqual(taken, set(range(21)))
        self.assertLessEqual(peak, 3)

    def test_more_workers_join_when_the_limit_rises(self):
        calls = [0]
        def rising(workers, running):
            calls[0] += 1
            return workers if calls[0] < 5 else 6
        done, peak, errors, _ = self.run_pool(list(range(200)), 2, rising)
        self.assertEqual(sorted(done), list(range(200)))
        self.assertGreater(peak, 2)
        self.assertLessEqual(peak, 6)


class BudgetTests(unittest.TestCase):
    def test_load_budget_leaves_room_for_busy_programs(self):
        self.assertEqual(load_budget(28, 0.0), 21)
        self.assertEqual(load_budget(28, 0.25), 14)
        self.assertEqual(load_budget(16, 0.9), 4)   # never under a quarter
        self.assertEqual(load_budget(2, 0.5), 1)

    def test_measured_budget_is_used_unless_overridden(self):
        self.assertEqual(cores(environ={'SKATE_SETUP_MEASURED_CORES': '9'}), 9)
        self.assertEqual(cores(environ={'SKATE_SETUP_MEASURED_CORES': '9', 'SKATE_SETUP_CORES': '4'}), 4)

    def test_customiser_limit_grows_only_after_the_maps(self):
        with tempfile.TemporaryDirectory() as tmp:
            marker = Path(tmp) / 'maps-done'
            env = {'SKATE_SETUP_MAPS_DONE': str(marker), 'SKATE_SETUP_CORES': '12'}
            self.assertEqual(customiser_limit(3, 3, env), 3)
            marker.touch()
            with patch('tools.asset_pipeline.install.available_memory', return_value=None):
                self.assertEqual(customiser_limit(3, 3, env), 11)
            with patch('tools.asset_pipeline.install.available_memory', return_value=6 * 1024**3):
                self.assertEqual(customiser_limit(3, 3, env), 5)  # 4 GiB spare = two more workers
            self.assertEqual(customiser_limit(3, 3, {**env, 'SKATE_SETUP_CUSTOMISER_WORKERS': '3'}), 3)


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

    def test_warm_up_splits_unconverted_models_and_textures_over_the_workers(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp)
            (cache / 'library').mkdir()
            (cache / 'library' / 'done_v4.glb').write_bytes(b'glb')
            catalog = {'materials': {}, 'components': [
                {'slot': 'Hat', 'models': [{'id': k, 'lods': []} for k in ('a', 'done', 'b', 'c')]},
                {'slot': 'Misc', 'models': [{'id': 'stamps', 'lods': []}]}]}
            (cache / 'catalog.json').write_text(json.dumps(catalog))
            sent = []
            def record(payload, work, workers):
                sent.extend(work)
                return [], set(range(len(work)))
            with patch('tools.asset_pipeline.customisation_library.requested_textures', return_value=['t1', 't2', 't3']), \
                 patch('tools.asset_pipeline.customisation_workers.run_shared', side_effect=record):
                warmed, _ = warm_up({'directory': str(cache)}, 2)
            self.assertEqual([b['kind'] for b in sent], ['models', 'textures'])  # models first
            self.assertEqual(sent[0]['items'], ['a', 'b', 'c'])
            self.assertEqual(sent[1]['items'], ['t1', 't2', 't3'])
            self.assertEqual(warmed, {'t1', 't2', 't3'})

    def test_derived_jobs_follow_the_material_loop(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            for key in ('hat', 'hair'):
                (out / f'{key}_v4.glb').write_bytes(b'glb')
            def mat(**tex):
                return {'textures': [{'channel': c, 'id': i} for c, i in tex.items()], 'flags': {}}
            def model(key, *mids):
                return {'id': key, 'lods': [{'index': 0, 'material_instances': [[{'id': m} for m in mids]]}]}
            catalog = {'materials': {'m1': mat(diffuse='d1', alpha='a1', normal='n1', specular='s1'),
                                     'm2': mat(diffuse='d2', alpha='a2'), 'm3': mat(normal='n3'), 'm4': mat(diffuse='d4', normal='n4')},
                       'components': [{'slot': 'Hat', 'models': [model('hat', 'm1', 'm3'), model('nomesh', 'm4')]},
                                      {'slot': 'Hair', 'models': [model('hair', 'm2', 'm1')]}]}
            self.assertEqual(derived_jobs(catalog, out), [('d1_a1', 'combined', 'd1', 'a1'), ('n1_normal', 'normal', 'n1'), ('s1_rough', 'rough', 's1')])


if __name__ == '__main__':
    unittest.main()
