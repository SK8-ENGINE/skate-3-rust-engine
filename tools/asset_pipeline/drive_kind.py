"""Whether a folder lives on a spinning hard drive (docs/hails-additions/08-setup-performance.md, "Hard drive note")."""
import os,plistlib,subprocess,sys
from pathlib import Path


def _existing(path):
    path=Path(os.path.realpath(path))
    while not path.exists() and path.parent!=path:path=path.parent
    return path


def _windows(path):
    import ctypes
    from ctypes import wintypes
    drive=os.path.splitdrive(str(path))[0]
    if len(drive)!=2 or drive[1]!=':':return None
    kernel=ctypes.WinDLL('kernel32',use_last_error=True)
    kernel.CreateFileW.restype=wintypes.HANDLE
    handle=kernel.CreateFileW('\\\\.\\'+drive,0,3,None,3,0,None)
    if handle in (None,wintypes.HANDLE(-1).value):return None
    try:
        query=(ctypes.c_uint32*3)(7,0,0)
        answer=(ctypes.c_uint32*3)()
        returned=wintypes.DWORD()
        if not kernel.DeviceIoControl(handle,0x2D1400,query,ctypes.sizeof(query),answer,ctypes.sizeof(answer),
                                      ctypes.byref(returned),None):return None
        if returned.value<9:return None
        return bool(answer[2]&0xFF)
    finally:kernel.CloseHandle(handle)


def _linux(path,sys_root='/sys'):
    device=os.stat(path).st_dev
    block=Path(sys_root)/'dev/block'/f'{os.major(device)}:{os.minor(device)}'
    try:block=block.resolve(strict=True)
    except OSError:return None
    for candidate in (block,block.parent):
        flag=candidate/'queue/rotational'
        if flag.is_file():return flag.read_text().strip()=='1'
    return None


def _mac(path):
    df=subprocess.run(['df','-P',str(path)],capture_output=True,text=True,timeout=10)
    lines=df.stdout.strip().splitlines()
    if df.returncode or len(lines)<2:return None
    info=subprocess.run(['diskutil','info','-plist',lines[1].split()[0]],capture_output=True,timeout=10)
    if info.returncode:return None
    solid=plistlib.loads(info.stdout).get('SolidState')
    return None if solid is None else not solid


def on_hard_drive(path):
    """True on a spinning hard drive, False on an SSD, None where the drive cannot be told."""
    try:
        path=_existing(path)
        if os.name=='nt':return _windows(path)
        if sys.platform=='darwin':return _mac(path)
        if sys.platform.startswith('linux'):return _linux(path)
    except (OSError,ValueError,subprocess.SubprocessError,plistlib.InvalidFileException):return None
    return None


HARD_DRIVE_NOTE=('Note: the install folder is on a hard drive (HDD). Setup and loading can take noticeably longer '
                 'there than on an SSD.')
