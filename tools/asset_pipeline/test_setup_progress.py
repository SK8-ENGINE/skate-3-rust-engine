"""The setup progress file the game's splash reads (setup_progress.py, design b172)."""
import json
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

from tools.asset_pipeline.setup_progress import ProgressWriter, counted


def lines(path):
    return [json.loads(l) for l in Path(path).read_text(encoding='utf-8').splitlines()]


class SetupProgress(unittest.TestCase):
    def test_steps_items_and_kinds_are_versioned_json_lines(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'p' / 'setup-progress.jsonl'
            echoed = []
            w = ProgressWriter(path, echo=echoed.append)
            w.write('start', refresh=True, groups=['maps'])
            w('Converting 2 maps with 2 workers')
            counted(w, 1, 2, 'Converted 1/2 maps: The Park')
            w.write('done', warnings=0, report='r.json')
            w.close()
            got = lines(path)
            self.assertEqual([g['kind'] for g in got], ['start', 'step', 'item', 'done'])
            self.assertTrue(all(g['v'] == 1 for g in got))
            self.assertEqual((got[2]['done'], got[2]['total']), (1, 2))
            self.assertEqual(echoed, ['Converting 2 maps with 2 workers', 'Converted 1/2 maps: The Park'])

    def test_a_plain_reporter_still_gets_the_counted_text(self):
        seen = []
        counted(seen.append, 3, 9, 'Converted 3/9 maps: Downtown')
        self.assertEqual(seen, ['Converted 3/9 maps: Downtown'])

    def test_windowless_refresh_writes_start_and_done_or_error(self):
        from tools import setup
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            args = types.SimpleNamespace(base=base, game_exe=base / 'skate3rust.exe')
            fake = types.ModuleType('fake')
            root = base / 'installations' / 'x'
            fake.install = lambda iso, b, exe, report, refresh: (report('Converting 1 maps with 1 workers'), root)[1]
            with patch.dict('sys.modules', {'tools.asset_pipeline.customiser_setup': fake}), \
                    patch('tools.asset_pipeline.validation_report.summary', return_value=[]):
                w = ProgressWriter(base / 'ok.jsonl')
                self.assertEqual(setup.windowless(args, w, base / 'default.xex', {'maps'}), 0)
                w.close()
            self.assertEqual([g['kind'] for g in lines(base / 'ok.jsonl')], ['start', 'step', 'done'])

            def broken(*_a, **_k):
                raise RuntimeError('disc unreadable')
            fake.install = broken
            with patch.dict('sys.modules', {'tools.asset_pipeline.customiser_setup': fake}):
                w = ProgressWriter(base / 'bad.jsonl')
                self.assertEqual(setup.windowless(args, w, base / 'default.xex', set()), 2)
                w.close()
            last = lines(base / 'bad.jsonl')[-1]
            self.assertEqual((last['kind'], last['text']), ('error', 'disc unreadable'))
            self.assertTrue((base / 'setup-error.log').is_file())


if __name__ == '__main__':
    unittest.main()
