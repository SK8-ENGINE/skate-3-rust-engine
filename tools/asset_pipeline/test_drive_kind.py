import os,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch

from tools.asset_pipeline import drive_kind


class DriveKind(unittest.TestCase):
    @unittest.skipUnless(hasattr(os,'major'),'device numbers are Unix only')
    def test_linux_reads_the_whole_disk_flag_for_a_partition(self):
        with tempfile.TemporaryDirectory() as root:
            device=os.stat(root).st_dev
            disk=Path(root)/'devices/sda';(disk/'queue').mkdir(parents=True);(disk/'sda1').mkdir()
            (disk/'queue/rotational').write_text('1\n')
            (Path(root)/'dev/block').mkdir(parents=True)
            os.symlink(disk/'sda1',Path(root)/'dev/block'/f'{os.major(device)}:{os.minor(device)}')
            self.assertTrue(drive_kind._linux(root,sys_root=root))
            (disk/'queue/rotational').write_text('0\n')
            self.assertFalse(drive_kind._linux(root,sys_root=root))

    def test_unknown_platform_or_failure_is_none(self):
        with patch.object(drive_kind.os,'name','posix'),patch.object(drive_kind.sys,'platform','sunos5'):
            self.assertIsNone(drive_kind.on_hard_drive('.'))
        with patch.object(drive_kind,'_existing',side_effect=OSError):
            self.assertIsNone(drive_kind.on_hard_drive('.'))

    @unittest.skipUnless(os.name=='nt','Windows drive query')
    def test_windows_answers_for_the_system_drive(self):
        self.assertIn(drive_kind.on_hard_drive(os.environ.get('SystemDrive','C:')+'\\'),(True,False))


if __name__=='__main__':unittest.main()
