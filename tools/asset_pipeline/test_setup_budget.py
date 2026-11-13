import io
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from . import install, setup_budget


class SetupBudgetTests(unittest.TestCase):
    def test_map_worker_override(self):
        self.assertEqual(setup_budget.map_workers(3, {}), 3)
        self.assertEqual(setup_budget.map_workers(3, {'SKATE_SETUP_MAP_WORKERS': '6'}), 6)
        for bad in ('0', '-2', 'many', ' '):
            self.assertEqual(setup_budget.map_workers(2, {'SKATE_SETUP_MAP_WORKERS': bad}), 2, bad)
        with mock.patch.dict(os.environ, {'SKATE_SETUP_MAP_WORKERS': '5'}):
            self.assertEqual(install.map_workers(), 5)

    def test_the_budget_is_three_quarters_of_the_cpus(self):
        for cpus, budget in ((1, 1), (2, 1), (4, 3), (8, 6), (16, 12), (28, 21), (128, 96)):
            self.assertEqual(setup_budget.cores(cpus, {}), budget, cpus)
        self.assertEqual(setup_budget.cores(28, {'SKATE_SETUP_CORES': '4'}), 4)
        self.assertEqual(setup_budget.cores(28, {'SKATE_SETUP_CORES': '0'}), 21)
        # Every pool stays inside the budget: three map jobs' threads plus the customiser's workers.
        for cpus in (2, 4, 8, 16, 28, 64):
            budget = setup_budget.cores(cpus, {})
            self.assertLessEqual(setup_budget.decode_workers(cpus, {}), budget)
            self.assertLessEqual(setup_budget.extract_threads(cpus, {}), budget)
            if budget >= 4:
                self.assertLessEqual(3 * setup_budget.job_threads(cpus, {}) + setup_budget.customiser_workers(cpus, {}), budget, cpus)

    def test_job_threads_scale_with_cpus_within_bounds(self):
        self.assertEqual(setup_budget.job_threads(1, {}), 1)
        self.assertEqual(setup_budget.job_threads(8, {}), 1)
        self.assertEqual(setup_budget.job_threads(16, {}), 3)
        self.assertEqual(setup_budget.job_threads(28, {}), 5)
        self.assertEqual(setup_budget.job_threads(128, {}), 29)
        self.assertEqual(setup_budget.job_threads(28, {'SKATE_SETUP_THREADS': '1'}), 1)
        self.assertEqual(setup_budget.job_threads(28, {'SKATE_SETUP_THREADS': '999'}), 32)
        self.assertEqual(setup_budget.job_threads(28, {'SKATE_SETUP_THREADS': 'x'}), 5)

    def test_priority_defaults_to_below_normal(self):
        self.assertEqual(setup_budget.priority_class({}), 0x4000)
        self.assertEqual(setup_budget.priority_class({'SKATE_SETUP_PRIORITY': 'Normal'}), 0x20)
        self.assertEqual(setup_budget.priority_class({'SKATE_SETUP_PRIORITY': 'idle'}), 0x40)
        self.assertEqual(setup_budget.priority_class({'SKATE_SETUP_PRIORITY': 'realtime'}), 0x4000)

    @unittest.skipUnless(os.name == 'nt', 'Windows priority classes')
    def test_lowered_priority_restores_the_previous_class(self):
        import ctypes
        kernel32 = ctypes.windll.kernel32
        kernel32.GetCurrentProcess.restype = ctypes.c_void_p
        kernel32.GetPriorityClass.argtypes = [ctypes.c_void_p]
        current = lambda: kernel32.GetPriorityClass(kernel32.GetCurrentProcess())
        before = current()
        with setup_budget.lowered_priority({}):
            self.assertEqual(current(), 0x4000)
            # A child started now inherits below normal.
            child = subprocess.run(['powershell', '-NoProfile', '-Command',
                                    '(Get-Process -Id $PID).PriorityClass'],
                                   capture_output=True, text=True, timeout=60)
            self.assertEqual(child.stdout.strip(), 'BelowNormal')
        self.assertEqual(current(), before)

    @unittest.skipUnless(os.name == 'nt', 'Windows creation flags')
    def test_conversion_processes_start_below_normal(self):
        seen = {}

        class Fake:
            def __init__(self, args, **kwargs):
                seen.update(kwargs)
                self.stdout = io.StringIO('converted\n')

            def __enter__(self):
                return self

            def __exit__(self, *_):
                return False

            def wait(self):
                return 0

        with tempfile.TemporaryDirectory() as work, mock.patch.object(subprocess, 'Popen', Fake):
            with (Path(work)/'log.txt').open('w', encoding='utf-8') as log:
                install.run(['tool.exe'], log, print)
            self.assertEqual(seen['creationflags'], subprocess.CREATE_NO_WINDOW | 0x4000)
            with mock.patch.dict(os.environ, {'SKATE_SETUP_PRIORITY': 'normal'}):
                with (Path(work)/'log.txt').open('w', encoding='utf-8') as log:
                    install.run(['tool.exe'], log, print)
            self.assertEqual(seen['creationflags'], subprocess.CREATE_NO_WINDOW | 0x20)


if __name__ == '__main__':
    unittest.main()
