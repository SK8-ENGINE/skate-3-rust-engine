"""setup.py brings its own Python packages on a source checkout (laptop 2026-10-17: Refresh Game Files stopped on
"No module named 'numpy'" because the game starts setup with a system Python)."""
import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

from tools import setup


class EnsureDependencies(unittest.TestCase):
    def test_packaged_setup_is_left_alone(self):
        with mock.patch.object(sys, 'frozen', True, create=True), mock.patch('subprocess.run') as run:
            self.assertIsNone(setup.ensure_dependencies())
        run.assert_not_called()

    def test_present_packages_need_nothing(self):
        with mock.patch.dict(sys.modules, {'numpy': mock.MagicMock(), 'PIL': mock.MagicMock()}), \
             mock.patch('subprocess.run') as run:
            self.assertIsNone(setup.ensure_dependencies())
        run.assert_not_called()

    def test_missing_packages_install_into_the_setup_venv_and_rerun(self):
        venv_python = setup.ROOT / '.local' / 'venv-setup' / 'Scripts' / 'python.exe'
        with mock.patch.dict(sys.modules, {'numpy': None}), \
             mock.patch.object(sys, 'argv', ['setup.py', '--base', 'data', '--refresh']), \
             mock.patch.object(Path, 'is_file', return_value=False), \
             mock.patch.object(Path, 'read_text', return_value='pyinstaller==6.19.0\nnumpy==2.2.6\nPillow==11.3.0\n'), \
             mock.patch('subprocess.run') as run, mock.patch('subprocess.call', return_value=0) as call:
            self.assertEqual(setup.ensure_dependencies(), 0)
        commands = [c.args[0] for c in run.call_args_list]
        self.assertEqual(commands[0][1:], ['-m', 'venv', str(venv_python.parents[1])])
        self.assertEqual(commands[1][-2:], ['numpy==2.2.6', 'Pillow==11.3.0'], 'pyinstaller is not a setup package')
        self.assertEqual(call.call_args.args[0][0], str(venv_python))
        self.assertEqual(call.call_args.args[0][-3:], ['--base', 'data', '--refresh'], 'the same arguments again')

    def test_a_failed_install_is_reported_not_a_crash(self):
        with mock.patch.dict(sys.modules, {'numpy': None, 'tkinter': None}), \
             mock.patch.object(Path, 'is_file', return_value=True), \
             mock.patch.object(Path, 'read_text', return_value='numpy==2.2.6\n'), \
             mock.patch('subprocess.run', side_effect=subprocess.CalledProcessError(1, 'pip')), \
             mock.patch('subprocess.call') as call:
            self.assertEqual(setup.ensure_dependencies(), 1)
        call.assert_not_called()


if __name__ == '__main__':
    unittest.main()
