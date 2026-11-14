import os
import tempfile
import unittest
from pathlib import Path
from tools.asset_pipeline.install import shared_changed
from tools.asset_pipeline.retained import _write
from tools.asset_pipeline.setup_state import receipt
from tools.asset_pipeline.group_receipts import valid_group_receipt as valid_receipt


class SharedLinks(unittest.TestCase):
    """A refresh hard-links the unchanged groups' files into the new installation (install.py)."""

    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        self.old, self.new = root/'old.json', root/'new.json'
        self.old.write_text('{"a": 1}', encoding='utf-8')
        os.link(self.old, self.new)
        self.shared = [(self.new, os.stat(self.new))]

    def test_untouched_links_pass(self):
        self.assertEqual(shared_changed(self.shared), [])

    def test_an_in_place_write_is_caught(self):
        os.utime(self.new, ns=(0, 10**9))
        with open(self.new, 'a', encoding='utf-8') as f:
            f.write(' ')
        self.assertEqual(shared_changed(self.shared), [self.new])
        self.assertEqual(self.old.read_text(encoding='utf-8'), '{"a": 1} ', 'an in-place write reaches the old copy')

    def test_replaced_or_removed_files_pass(self):
        self.new.unlink()
        self.assertEqual(shared_changed(self.shared), [])
        self.new.write_text('{"b": 2}', encoding='utf-8')
        self.assertEqual(shared_changed(self.shared), [])

    def test_only_groups_no_other_step_writes_are_linked(self):
        # The maps step writes core's stock/dlc rows and the living world's hand props in place (laptop 2026-10-23).
        from tools.asset_pipeline.install import LINKED_GROUPS
        from tools.asset_pipeline.group_receipts import ROOTS
        written_by_other_steps = ('assets/private/stock', 'assets/private/living_world')
        for group in LINKED_GROUPS:
            for root in ROOTS[group]:
                self.assertFalse(root.startswith(written_by_other_steps), root)
        self.assertNotIn('core', LINKED_GROUPS)
        self.assertNotIn('livingworld', LINKED_GROUPS)

    def test_retained_write_leaves_the_previous_installation_alone(self):
        _write(self.new, {'b': 2}, None)
        self.assertEqual(self.old.read_text(encoding='utf-8'), '{"a": 1}')
        self.assertEqual(self.new.read_text(encoding='utf-8'), '{"b": 2}')
        self.assertEqual(shared_changed(self.shared), [])


class MapWriters(unittest.TestCase):
    """A maps rebuild hard-links the previous maps into the new installation; the writers must make new files."""

    def test_writing_a_linked_irradiance_file_leaves_the_previous_one(self):
        import json
        from tools.asset_pipeline.irradiance import write
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            previous, new = temp/'old.irradiance', temp/'new.irradiance'
            previous.write_bytes(b'previous installation')
            os.link(previous, new)
            manifest = temp/'manifest.json'
            manifest.write_text(json.dumps({'other_presentation_assets': []}))
            linked = [(new, os.stat(new))]
            write(manifest, new)
            self.assertEqual(previous.read_bytes(), b'previous installation')
            self.assertEqual(new.read_bytes()[:4], b'SHP1')
            self.assertEqual(shared_changed(linked), [])


class ReceiptTimes(unittest.TestCase):
    """damaged() skips the re-hash when size and modification time match (group_receipts.valid_group_receipt)."""

    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.file = self.root/'audio.bnk'
        self.file.write_bytes(b'good')
        os.utime(self.file, ns=(0, 10**18))
        self.files = receipt(self.root, [self.file])
        self.files['audio.bnk']['mtime_ns'] = os.stat(self.file).st_mtime_ns  # as group_receipts.record

    def corrupt(self, keep_time):
        self.file.write_bytes(b'evil')
        if keep_time:
            os.utime(self.file, ns=(0, 10**18))

    def test_the_entry_records_the_time(self):
        self.assertEqual(self.files['audio.bnk']['mtime_ns'], 10**18)

    def test_a_newer_time_is_hashed_and_caught(self):
        self.corrupt(keep_time=False)
        self.assertFalse(valid_receipt(self.root, self.files))

    def test_same_size_and_time_is_trusted_without_reading(self):
        # The accepted trade-off: a same-size change that keeps the time is not seen.
        self.corrupt(keep_time=True)
        self.assertTrue(valid_receipt(self.root, self.files))

    def test_an_entry_without_a_time_is_hashed_and_gains_it(self):
        del self.files['audio.bnk']['mtime_ns']
        self.assertTrue(valid_receipt(self.root, self.files))
        self.assertEqual(self.files['audio.bnk']['mtime_ns'], 10**18)
        del self.files['audio.bnk']['mtime_ns']
        self.corrupt(keep_time=True)
        self.assertFalse(valid_receipt(self.root, self.files))
        self.assertNotIn('mtime_ns', self.files['audio.bnk'])


if __name__ == '__main__':
    unittest.main()
