"""Export the HUDs, the ped chase alerts, the replay shake clips and the front-end menu movies used by the engine, as one automatic setup stage.

Only runtime manifests and referenced RGBA payloads enter the installation.
Extraction intermediates stay in setup's conversion workspace and are removed
by its normal successful-install cleanup. No audio or full-disc extraction.
"""
import argparse
from pathlib import Path
import sys
import tempfile

# The repo root too, after tools/: `tools.*` imports (the DLC package reader) as in prepare_trick_guide_scene.py.
sys.path.insert(1, str(Path(__file__).resolve().parents[1]))

from prepare_hud import prepare as prepare_scoring
from extract_session_marker import prepare as prepare_marker
from install_prepared_hud import install


def prepare(game, assets, work, dlc=None):
    # Retail texture names plus the install/generation prefix can exceed legacy
    # Windows path limits. Only compact runtime files belong in the installation.
    with tempfile.TemporaryDirectory(prefix='sk8hud-') as temporary:
        return prepare_in_workspace(game, assets, Path(temporary), dlc)


def prepare_in_workspace(game, assets, work, dlc=None):
    from asset_pipeline.optional_content import CONTENT_ERRORS, note
    from install_prepared_hud import runtime_files
    scoring = work / 'scoring'
    marker = work / 'session-marker'
    files = {}
    for name, source, action, is_marker in (
        ('hud', scoring, lambda: prepare_scoring(game, scoring, assets/'private/stock/skater-collections.json'), False),
        ('session-marker', marker, lambda: prepare_marker(game, marker), True)):
        availability=assets/'private'/(name+'-availability.json')
        try:
            action()
            runtime_files(source, is_marker)
        except CONTENT_ERRORS as error:
            try:
                runtime_files(assets/'private'/name,is_marker)
                retained=True
            except CONTENT_ERRORS:
                retained=False
                # Broken artwork must not be loaded as if it were complete.
                (assets/'private'/name/('hud.json' if is_marker else 'runtime/trickdisplay.json')).unlink(missing_ok=True)
            note(availability,name,error,retained)
            continue
        files.update(install(assets,None if is_marker else source,source if is_marker else None,replace=True))
        availability.unlink(missing_ok=True)
    # Front-end menu movies (APT) for the retail menus, same disc archives and fonts.
    import prepare_menu_movies as menus
    availability=assets/'private'/'menu-movies-availability.json'
    try:
        menus.prepare(game, work/'menus', assets/'private/stock/skater-collections.json', dlc=dlc)
        files.update(menus.install(assets, work/'menus'/'runtime'))
        availability.unlink(missing_ok=True)
    except CONTENT_ERRORS as error:
        try:
            menus.runtime_files(assets/'private'/'menu-movies')
            retained=True
        except CONTENT_ERRORS:
            retained=False
            (assets/'private'/'menu-movies'/'manifest.json').unlink(missing_ok=True)
        note(availability,'menu-movies',error,retained)
    # The alerts over peds that warn or chase the player (retail ChaseArrow textures, b191).
    import prepare_chase_arrows as arrows
    availability=assets/'private'/'chase-arrows-availability.json'
    try:
        arrows.prepare(game, work/'chase-arrows')
        files.update(arrows.install(assets, work/'chase-arrows'))
        availability.unlink(missing_ok=True)
    except CONTENT_ERRORS as error:
        try:
            arrows.runtime_files(assets/'private'/'hud'/'chase-arrows')
            retained=True
        except CONTENT_ERRORS:
            retained=False
        note(availability,'chase-arrows',error,retained)
    # The replay editor's camera shake clips (ICELite.bin in scene.big, research b400).
    import prepare_replay_shake as shake
    availability=assets/'private'/'replay-shake-availability.json'
    try:
        shake.prepare(game, work/'replay-shake')
        files.update(shake.install(assets, work/'replay-shake'))
        availability.unlink(missing_ok=True)
    except CONTENT_ERRORS as error:
        try:
            shake.runtime_files(assets/'private'/'hud'/'replay-shake')
            retained=True
        except CONTENT_ERRORS:
            retained=False
        note(availability,'replay-shake',error,retained)
    print(f'Original runtime HUDs ready: {len(files)} verified files', flush=True)
    return files


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--dlc', type=Path, help="the player's DLC package folder (optional)")
    args = parser.parse_args()
    prepare(args.game, args.assets, args.work, args.dlc)
