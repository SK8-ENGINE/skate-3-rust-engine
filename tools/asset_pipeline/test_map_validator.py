import io
import json
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

from . import install
from .validation_report import summary

# Speaks the skate3rust --validate-maps protocol (crates/skate-game/src/map_validation.rs).
FAKE = textwrap.dedent('''
    import json, sys
    mode = sys.argv[1]
    if mode == 'no-ready':
        print('Unknown argument "--validate-maps"', file=sys.stderr); sys.exit(1)
    print('SKATE_VALIDATOR_READY seconds=0.001', flush=True)
    for line in sys.stdin:
        path = line.strip()
        print('SKATE_RWCM_READY triangles=12 (log line)', file=sys.stderr, flush=True)
        if 'die' in path:
            sys.exit(3)
        result = {'path': path, 'ok': 'error' not in path, 'seconds': 0.25,
                  'errors': ['bad map'] if 'error' in path else [],
                  'warnings': ['no collision within 10 m below the spawn'] if 'warn' in path else []}
        print('noise before the result', flush=True)
        print('SKATE_MAP_CHECK ' + json.dumps(result), flush=True)
''')


class MapValidatorTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory()
        self.root = Path(self.work.name)
        self.fake = self.root / 'fake_validator.py'
        self.fake.write_text(FAKE, encoding='utf-8')
        self.log = io.StringIO()
        self.reports = []

    def tearDown(self):
        self.work.cleanup()

    def start(self, mode='ok'):
        return install.MapValidator([sys.executable, self.fake, mode], self.log)

    def test_results_warnings_and_log_copy(self):
        validator = self.start()
        try:
            clean = validator.check(self.root / 'maps/Clean.skate')
            warned = validator.check(self.root / 'maps/warn.skate')
            failed = validator.check('error.skate')
        finally:
            validator.close()
        self.assertTrue(clean['ok'])
        self.assertEqual(clean['warnings'], [])
        self.assertEqual(warned['warnings'], ['no collision within 10 m below the spawn'])
        self.assertFalse(failed['ok'])
        self.assertIn('SKATE_VALIDATOR_READY', self.log.getvalue())
        self.assertIn('SKATE_MAP_CHECK', self.log.getvalue())
        self.assertIn('SKATE_RWCM_READY', self.log.getvalue())
        self.assertEqual(validator.process.returncode, 0)

    def test_validator_that_never_becomes_ready_falls_back(self):
        with self.assertRaisesRegex(RuntimeError, 'did not start'):
            self.start('no-ready')
        self.assertIsNone(install.start_validator(self.root / 'missing.exe', self.root, self.log, self.reports.append))
        self.assertIn('validating with --check-assets per map', self.reports[0])

    def test_validator_death_is_a_runtime_error(self):
        validator = self.start()
        with self.assertRaisesRegex(RuntimeError, 'exited'):
            validator.check('die.skate')
        validator.close()

    def test_paths_cannot_inject_extra_requests(self):
        validator = self.start()
        try:
            with self.assertRaises(ValueError):
                validator.check('a.skate\nb.skate')
        finally:
            validator.close()

    def test_warnings_are_recorded_and_reported_and_clean_runs_clear_them(self):
        stage = self.root / 'stage'
        private = stage / 'assets/private'
        entry = {'name': 'MegaPark', 'path': 'maps/MegaPark.skate', 'phase_seconds': {'prepare': 1.0}}
        install.record_validation(private, entry, {'ok': True, 'seconds': 0.25, 'warnings': ['spawn over void']},
                                  self.reports.append)
        self.assertEqual(entry['phase_seconds']['validate'], 0.25)
        warnings = summary(stage)
        self.assertEqual([w['status'] for w in warnings], ['warning'])
        self.assertEqual(warnings[0]['warnings'], ['spawn over void'])
        self.assertTrue((stage / 'setup-report.json').is_file())
        install.record_validation(private, entry, {'ok': True, 'seconds': 0.2, 'warnings': []}, self.reports.append)
        self.assertEqual(summary(stage), [])
        self.assertFalse((stage / 'setup-report.json').exists())
        # Fallback path (no validator) records nothing and keeps timings intact.
        install.record_validation(private, entry, None, self.reports.append)
        self.assertEqual(entry['phase_seconds']['validate'], 0.2)


class CustomiserOverlapTests(unittest.TestCase):
    def test_background_join_waits_and_reraises(self):
        done = []
        install.Background(lambda: done.append(1), 'ok').join()
        self.assertEqual(done, [1])
        failing = install.Background(lambda: (_ for _ in ()).throw(RuntimeError('customiser failed')), 'bad')
        with self.assertRaisesRegex(RuntimeError, 'customiser failed'):
            failing.join()

    def test_overlap_needs_one_more_slot_than_the_map_workers(self):
        original = install.available_memory
        try:
            install.available_memory = lambda: (2 + 4 * 3) * install.GIB      # 3 workers + customiser fit
            self.assertTrue(install.overlap_customiser(3))
            install.available_memory = lambda: (2 + 4 * 3) * install.GIB - 1  # one byte short
            self.assertFalse(install.overlap_customiser(3))
            install.available_memory = lambda: None                           # cannot query: allowed
            self.assertTrue(install.overlap_customiser(3))
        finally:
            install.available_memory = original


if __name__ == '__main__':
    unittest.main()
