import subprocess,unittest
from unittest.mock import patch

from tools.asset_pipeline import install,setup_budget

GIB=1024**3


class LowMemory(unittest.TestCase):
    def test_linux_reads_mem_available(self):
        meminfo='MemTotal:       8000000 kB\nMemFree:         100000 kB\nMemAvailable:    3145728 kB\n'
        with patch.object(install.sys,'platform','linux'),patch.object(install.Path,'read_text',return_value=meminfo):
            self.assertEqual(install.available_memory(),3*GIB)

    def test_mac_counts_free_inactive_and_purgeable_pages(self):
        vm_stat=('Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:      1000.\n'
                 'Pages active:    5000.\nPages inactive:  2000.\nPages purgeable: 500.\n')
        done=subprocess.CompletedProcess(['vm_stat'],0,vm_stat,'')
        with patch.object(install.sys,'platform','darwin'),patch.object(install.subprocess,'run',return_value=done):
            self.assertEqual(install.available_memory(),3500*16384)

    def test_customiser_workers_fit_free_memory(self):
        with patch.object(install,'available_memory',return_value=None):
            unlimited=setup_budget.customiser_workers()
        with patch.object(install,'available_memory',return_value=6*GIB):
            self.assertEqual(setup_budget.customiser_workers(),min(unlimited,2))
        with patch.object(install,'available_memory',return_value=GIB):
            self.assertEqual(setup_budget.customiser_workers(),1)


if __name__=='__main__':unittest.main()
