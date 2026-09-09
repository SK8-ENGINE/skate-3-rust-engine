"""Export the two HUDs used by the engine, as one automatic setup stage.

Only runtime manifests and referenced RGBA payloads enter the installation.
Extraction intermediates stay in setup's conversion workspace and are removed
by its normal successful-install cleanup. No audio or full-disc extraction.
"""
import argparse
from pathlib import Path

from prepare_hud import prepare as prepare_scoring
from extract_session_marker import prepare as prepare_marker
from install_prepared_hud import install


def prepare(game, assets, work):
    scoring = work / 'scoring'
    marker = work / 'session-marker'
    prepare_scoring(game, scoring, assets / 'private/stock/skater-collections.json')
    prepare_marker(game, marker)
    files = install(assets, scoring, marker)
    print(f'Original runtime HUDs ready: {len(files)} verified files', flush=True)
    return files


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    args = parser.parse_args()
    prepare(args.game, args.assets, args.work)
