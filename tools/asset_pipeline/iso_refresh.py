"""Refresh Game Files from an ISO without extracting the whole disc when nothing needs rebuilding (doc 08 "ISO refresh
without extracting the disc").

Only the files that check reads come out of the image (xiso.py): default.xex for the same-edition check and the
character customiser's source archives (customiser_setup.source_fingerprint). The source folder is marked partial;
when install._install_stages finds a group to rebuild, or the customiser set is not current for this code and disc, it
raises NeedsFullSource before anything is written, and the refresh runs again with the whole disc extracted as before.
Kept out of customiser_*.py and owned_game/, which are part of the customiser's fingerprint.
"""
import json
import re
import tempfile
from pathlib import Path

CHECKED = ('default.xex', 'data/content/createacharacter.big', 'data/content/marquee.big', 'data/big/db.big',
           'data/big/miscload.big')
PARTIAL = '.partial-source'


class NeedsFullSource(Exception):
    """The refresh has work that reads more of the disc than the partial source holds."""


def customiser_current(source, assets):
    """customiser_setup._prepare's early return: the current set is complete for this code and this disc."""
    from .customiser_setup import fingerprint, source_fingerprint
    from . import customiser_cache as cache
    base = assets/'private/customisation'
    try:
        saved = json.loads((base/'current.json').read_text())
    except (OSError, ValueError):
        return False
    return bool(isinstance(saved, dict) and isinstance(saved.get('set'), str)
                and re.fullmatch('[0-9a-f]{32}', saved['set']) and saved.get('fingerprint') == fingerprint()
                and saved.get('source') == source_fingerprint(source)
                and all((base/'sets'/saved['set']/name).is_file() for name in
                        ('library-v3.json', 'extra-menu.json', 'native-lighting.json', 'native-roster/complete.json'))
                and cache.complete(base/'sets'/saved['set']))


def finish(source, assets):
    """In place of customiser_setup.prepare on a partial source: its success path when the set is current."""
    if not customiser_current(source, assets):
        raise NeedsFullSource('character customiser')
    (assets/'private/customisation/customiser-availability.json').unlink(missing_ok=True)


def install(iso, base, game_exe, report):
    from . import install as core
    from .customiser_setup import install as full
    from .setup_state import setup_lock
    from .xiso import Xiso
    selected = Path(iso).resolve()
    try:
        with setup_lock(base), tempfile.TemporaryDirectory(prefix='character-source-', dir=base.parent) as temp:
            source = Path(temp)/'disc'
            report('Reading the checked files from the ISO')
            if len(Xiso(selected).extract(CHECKED, source)) != len(CHECKED):
                raise NeedsFullSource('a checked disc file is missing')
            (source/PARTIAL).write_text('only the files a refresh checks\n')
            return core._install(selected, base, game_exe, report, game_root=source, refresh=True,
                                 finalize=lambda stage: finish(source, stage/'assets'))
    except NeedsFullSource as reason:
        report(f'Extracting the whole disc ({reason})')
    except ValueError:
        report('Extracting the whole disc (the ISO could not be read directly)')
    return full(selected, base, game_exe, report, refresh=True)
