"""Content identities for independently refreshable owned-disc asset groups."""
import argparse
import hashlib
import json
from pathlib import Path
import re

GROUPS = ('core', 'hud', 'character', 'environment', 'maps')
COMMON = ('owned_game/**/*.py', 'asset_pipeline/fast_refpack.py', 'asset_pipeline/install.py',
          'asset_pipeline/versions.py', 'asset_pipeline/refpack_native.rs')
PARSERS = ('vendor/utt/**/*.py', 'vendor/university/**/*.py', 'vendor/utt/**/*.json', 'vendor/university/**/*.json')
SOURCES = {
    'core': ('asset_pipeline/vlt.py', 'asset_pipeline/names.txt', 'asset_pipeline/physics_skeleton.py'),
    'hud': ('prepare_hud.py', 'prepare_runtime_huds.py', 'install_prepared_hud.py',
            'extract_session_marker.py', 'vendor/skate3_ui/**/*.py', 'vendor/skate3_ui/**/*.json'),
    'character': PARSERS + ('extract_default_skater.py', 'default_skater_retail_manifest.json',
                  'asset_pipeline/character*.py'),
    'environment': PARSERS + ('asset_pipeline/sky.py', 'asset_pipeline/backdrop.py',
                  'asset_pipeline/render_parameters.py', 'asset_pipeline/teleports.py',
                  'asset_pipeline/environment.py', 'asset_pipeline/map_writer.py',
                  'asset_pipeline/retail_material.py', 'asset_pipeline/irradiance.py'),
    'maps': PARSERS + ('asset_pipeline/map*.py', 'asset_pipeline/dynamic_props.py',
             'asset_pipeline/environment.py', 'asset_pipeline/irradiance.py',
             'asset_pipeline/retail_material.py', 'asset_pipeline/backdrop.py', 'asset_pipeline/sky.py'),
}


def fingerprints(tools=None):
    tools = tools or Path(__file__).resolve().parents[1]
    result = {}
    for group, patterns in SOURCES.items():
        paths = {p for pattern in COMMON + patterns for p in tools.glob(pattern)
                 if p.is_file() and not p.name.startswith('test_') and '__pycache__' not in p.parts}
        digest = hashlib.sha256()
        for path in sorted(paths, key=lambda p: p.relative_to(tools).as_posix()):
            digest.update(path.relative_to(tools).as_posix().encode() + b'\0')
            digest.update(hashlib.sha256(path.read_bytes()).digest())
        result[group] = digest.hexdigest()
    return result


def changed_groups(previous, current):
    changed = {name for name in GROUPS if previous.get(name) != current[name]}
    # Other exports consume the core VLT/skeleton/animation data.
    if 'core' in changed:
        changed.update(GROUPS)
    return changed


def installed(base):
    marker = base/'installation.json'
    if not marker.is_file():
        return None
    value = json.loads(marker.read_text(encoding='utf-8-sig'))
    directory = value.get('directory', '')
    if value.get('version') != 1 or not re.fullmatch(r'installations/[0-9a-f]{32}', directory):
        raise ValueError('Invalid installation record')
    root = (base/directory).resolve()
    if not root.is_relative_to(base.resolve()):
        raise ValueError('Installation escapes its package data directory')
    if not (root/'assets/private/game.json').is_file():
        return None
    return root, value


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--tools', type=Path)
    print(json.dumps(fingerprints(parser.parse_args().tools)))
