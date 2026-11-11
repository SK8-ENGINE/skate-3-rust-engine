import io,os,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch

from tools.asset_pipeline import install


class DevGameExe(unittest.TestCase):
    def test_deps_folder_and_toolchain_of_a_cargo_dev_build_are_found(self):
        with tempfile.TemporaryDirectory() as root:
            exe=Path(root)/'skate3rust.exe';exe.write_bytes(b'')
            self.assertEqual(install.dev_library_dirs(exe),[])
            deps=Path(root)/'deps';deps.mkdir()
            self.assertEqual(install.dev_library_dirs(exe),[])
            (deps/'bevy_dylib-4567.dll').write_bytes(b'')
            toolchain=Path(root)/'toolchain'
            with patch.object(install,'rust_sysroot_libs',return_value=toolchain):
                self.assertEqual(install.dev_library_dirs(exe),[deps.resolve(),toolchain])
            with patch.object(install,'rust_sysroot_libs',return_value=None):
                self.assertEqual(install.dev_library_dirs(exe),[deps.resolve()])

    def test_spawned_dev_build_gets_deps_first_on_the_library_path(self):
        with tempfile.TemporaryDirectory() as root:
            exe=Path(root)/'skate3rust.exe';exe.write_bytes(b'')
            deps=Path(root)/'deps';deps.mkdir()
            (deps/'bevy_dylib-4567.dll').write_bytes(b'')
            with patch.object(install,'rust_sysroot_libs',return_value=Path(root)/'toolchain'), \
                 patch.object(install.subprocess,'Popen') as popen:
                install.spawn([exe,'--check-assets'])
            env=popen.call_args.kwargs['env']
            self.assertEqual(env[install.LIBRARY_PATH].split(os.pathsep)[:2],[str(deps.resolve()),str(Path(root)/'toolchain')])
            with patch.object(install.subprocess,'Popen') as popen:
                install.spawn([sys.executable,'-c','pass'])
            self.assertNotIn('env',popen.call_args.kwargs)

    def test_missing_dll_exit_says_so(self):
        class Process:
            stdout=io.StringIO('')
            def __enter__(self):return self
            def __exit__(self,*_):return False
            def wait(self):return -1073741515
        log=io.StringIO();log.name='setup.log'
        with patch.object(install,'spawn',return_value=Process()):
            with self.assertRaisesRegex(RuntimeError,'DLL it needs is missing'):
                install.run(['skate3rust.exe','--check-assets'],log,print)


if __name__=='__main__':unittest.main()
