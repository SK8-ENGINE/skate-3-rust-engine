"""The release package's identity and program manifest (release.json), checked before a package is published.

The Skate launcher installs and updates releases (tools/skate-launcher: the full package or patches, doc 44a
"Patch updates"); this module only validates what the publish scripts upload. User data is never release-owned.
"""
import os
import re

DEFAULT_REPO = 'SK8-ENGINE/skate-3-rust-engine'
PACKAGE = 'skate3rust-windows-x64.zip'
PREFIX = 'skate3rust-windows-x64/'
PROTECTED = {'data', 'assets', 'maps', 'logs', 'saves', 'settings', 'mods', 'custom-models', 'screenshots'}
REQUIRED = {'skate3rust.exe', 'support/skate3setup.exe'}
RESERVED = {'CON', 'PRN', 'AUX', 'NUL', *(f'COM{i}' for i in range(10)), *(f'LPT{i}' for i in range(10))}


def release_repository():
    # A fork's CI publish scripts set SKATE3RUST_REPOSITORY to its owner/repo.
    repo = os.environ.get('SKATE3RUST_REPOSITORY') or DEFAULT_REPO
    if not re.fullmatch(r'[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}', repo):
        raise ValueError('Invalid release repository')
    return repo


REPO = release_repository()


def safe_name(name):
    if not isinstance(name, str) or not name or '\\' in name or ':' in name or name.startswith('/'):
        raise ValueError('Unsafe program path')
    parts = name.split('/')
    if any(not p or p in ('.', '..') or p[-1] in ' .' or any(ord(c) < 32 for c in p)
           or p.split('.')[0].upper() in RESERVED for p in parts):
        raise ValueError('Unsafe program path')
    if parts[0].lower() in PROTECTED or parts[0].startswith('.'):
        raise ValueError('Release cannot manage user data')
    return name


def manifest_files(meta):
    files = meta.get('files')
    if not isinstance(files, dict) or not REQUIRED <= files.keys() or len(files) > 20000:
        raise ValueError('Missing program hashes')
    seen = set()
    for name, digest in files.items():
        safe_name(name)
        if (name.lower() == 'release.json' or name.lower() in seen or not isinstance(digest, str)
                or not re.fullmatch('[0-9a-f]{64}', digest)):
            raise ValueError('Invalid program manifest')
        seen.add(name.lower())
    for name in seen:
        if any('/'.join(name.split('/')[:i]) in seen for i in range(1, len(name.split('/')))):
            raise ValueError('Program file/directory conflict')
    return files


def identity(m):
    if not isinstance(m, dict):
        raise ValueError('Invalid release metadata')
    build = m.get('build')
    tag = m.get('tag')
    if (m.get('schema') != 1 or m.get('target') != 'windows-x64'
            or m.get('repository') != REPO or not isinstance(build, int)
            or (build <= 0 and tag != 'development') or not isinstance(tag, str)
            or not isinstance(m.get('revision'), str)
            or not re.fullmatch(r'[0-9a-f]{40}', m['revision'])):
        raise ValueError('Incompatible release metadata')
    return build


def program_metadata(m):
    identity(m)
    manifest_files(m)


def package_name(meta):
    # Rolling asset names come from a validated integer, never a path from release notes or a manifest field.
    return f'skate3rust-windows-x64-build-{identity(meta)}.zip' if meta['tag'] == 'experimental' else PACKAGE
