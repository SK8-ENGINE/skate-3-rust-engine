"""Platform-aware FBX2glTF lookup.

Windows bundles ``FBX2glTF.exe``; macOS/Linux bundle an extension-less
``FBX2glTF`` next to the same ``tools/`` directory. Hashes pin v0.9.7,
the release the retail-pipeline checks assert against.
"""
import sys
from pathlib import Path

VERSION = '0.9.7'
URLS = {
    'win32': 'https://github.com/facebookincubator/FBX2glTF/releases/download/v0.9.7/FBX2glTF-windows-x64.exe',
    'darwin': 'https://github.com/facebookincubator/FBX2glTF/releases/download/v0.9.7/FBX2glTF-darwin-x64',
}
SHA = {
    'win32': '8d90fb5e0a8d186a3d9a7ff8c75eaee541c3975ce4df0d80351f20092ae0877f',
    'darwin': 'f82383ae4185c39f991b479b04ecce104f02e70c12a035ed31fc469e6f74a3fd',
}

# The darwin asset is an x86_64 Mach-O binary; Apple Silicon Macs run it
# through Rosetta 2 (see scripts/prepare-character-importer-macos.sh).


def bundled_name():
    if sys.platform == 'win32':
        return 'FBX2glTF.exe'
    if sys.platform == 'darwin':
        return 'FBX2glTF'
    raise RuntimeError('FBX character import is supported on Windows and macOS only')


def bundled_path(root):
    return Path(root) / 'tools' / bundled_name()


def missing_message():
    if sys.platform == 'win32':
        return 'FBX2glTF.exe is missing; run the converter setup'
    return (bundled_name() + ' is missing; fetch it with '
            'scripts/prepare-character-importer-macos.sh (macOS) or pass --fbx-tool')
