"""Validate prepared groups without treating user files as extractor output."""
import json
import stat
from .setup_state import receipt, valid_receipt

ROOTS = {
    'core': ('assets/private/stock/data/anim', 'assets/private/stock/data/state',
             'assets/private/stock/data/config', 'assets/private/stock/data/cacrecipes',
             'assets/private/stock/skater-collections.json', 'assets/private/stock/physics-skeletons.json'),
    'hud': ('assets/private/hud', 'assets/private/session-marker',
            'assets/private/hud-availability.json', 'assets/private/session-marker-availability.json'),
    'character': ('assets/private/game.json', 'assets/private/skater.glb', 'assets/private/default_skater',
                  'assets/private/native-character', 'assets/private/character-lighting.json'),
    'environment': ('assets/private/native-skies', 'assets/private/native-backdrops',
                    'assets/private/render-parameters.json', 'assets/private/exposure.json',
                    'assets/private/exposure-profiles.json', 'assets/private/teleports.json', 'assets/private/environment-status'),
    'maps': ('assets/private/native-props', 'maps.json', 'assets/private/map-status'),
    'audio': ('assets/private/audio', 'assets/private/audio-availability.json'),
    'livingworld': ('assets/private/living_world', 'assets/private/living_world-availability.json'),
}


def files(root, group):
    result = []
    for name in ROOTS[group]:
        path = root/name
        result.extend(path.rglob('*') if path.is_dir() else [path])
    if group == 'core':
        stock = root/'assets/private/stock'
        result.extend(p for p in stock.rglob('*')
                      if not p.is_relative_to(stock/'data/content/createacharacter'))
    if group == 'maps':
        for item in json.loads((root/'maps.json').read_text()):
            path = (root/item['path']).resolve()
            if not path.is_relative_to((root/'maps').resolve()):
                raise ValueError('Invalid prepared map path')
            result.append(path)
    return result


def _receipt(root, paths):
    """setup_state.receipt (the same entries in the same order) with the files hashed on the setup's threads
    (hashlib releases the GIL; a 19 GB installation took 20 s on one thread)."""
    from concurrent.futures import ThreadPoolExecutor
    from .setup_budget import cores
    from .setup_state import sha
    root = root.resolve()
    paths = [p for p in sorted({p.resolve() for p in paths}) if p.is_file()]
    with ThreadPoolExecutor(max(1, min(16, cores()))) as pool:
        digests = list(pool.map(sha, paths))
    return {p.relative_to(root).as_posix(): {'size': p.stat().st_size, 'sha256': d} for p, d in zip(paths, digests)}


def record(root, group):
    files_ = _receipt(root, files(root, group))
    for name, item in files_.items():
        item['mtime_ns'] = (root.resolve()/name).stat().st_mtime_ns
    return files_


def valid_group_receipt(root, files_):
    """valid_receipt, but a file whose size and recorded modification time both match counts as unchanged without
    reading it (user 2026-10-23: skip the re-hash). Hard links and copy2 keep the time, so an entry stays valid in the
    next installation. Entries without a time are hashed and gain it when they match. Kept out of setup_state.py: that
    file is part of the customiser's fingerprint."""
    if not isinstance(files_, dict) or not files_:
        return False
    hashed = {}
    try:
        # The root once, not per file; one stat per file (a refresh checks ~37k files, doc 08 "Refresh check cost").
        inside = root.resolve()
        for name, item in files_.items():
            path = (root/name).resolve()
            if not path.is_relative_to(inside):
                return False
            status = path.stat()
            if not stat.S_ISREG(status.st_mode):
                return False
            if status.st_size != item['size']:
                return False
            if item.get('mtime_ns') != status.st_mtime_ns:
                hashed[name] = item
    except (OSError, KeyError, TypeError, ValueError):
        return False
    if hashed and not valid_receipt(root, hashed):
        return False
    for name in hashed:
        hashed[name]['mtime_ns'] = (root/name).resolve().stat().st_mtime_ns
    return True


def damaged(root, marker, exclude=(), report=None):
    saved = marker.get('outputs', {})
    result = set()
    checked = [group for group in ROOTS if group not in exclude]
    for group, roots in ROOTS.items():
        if group in exclude:continue
        # Every installed file is checked (size and time; a changed one is hashed): tens of thousands of files,
        # so setup's window shows which group it is on.
        if report:report(f'Checking your installed game files: {group} ({checked.index(group)+1} of {len(checked)})...')
        if group in saved:
            if not valid_group_receipt(root, saved[group]):result.add(group)
        else:
            # Legacy packages have no receipts: require their declared roots
            # and verify the map checksums they already recorded. The runtime
            # validator also checks legacy core inputs before publication.
            if any(not (root/name).exists() for name in roots if not name.endswith(('-availability.json', '-status'))):result.add(group)
            if group == 'hud' and group not in result:
                from tools.install_prepared_hud import runtime_files
                try:
                    runtime_files(root/'assets/private/hud')
                    runtime_files(root/'assets/private/session-marker', marker=True)
                except (OSError, ValueError, KeyError, TypeError):result.add(group)
            if group == 'maps' and group not in result:
                try:
                    maps = json.loads((root/'maps.json').read_text())
                    checks = {m['path']: {'size': (root/m['path']).stat().st_size,
                                         'sha256': m['sha256']} for m in maps}
                    if not valid_receipt(root, checks):result.add(group)
                except (OSError, ValueError, KeyError, TypeError):result.add(group)
    if 'core' in result:result.update(ROOTS)
    return result
