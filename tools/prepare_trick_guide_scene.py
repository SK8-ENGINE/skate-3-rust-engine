"""Setup task: the Trick Guide demo scene (clips + set) into the hud group (asset_pipeline/scene_clips.py)."""
import argparse
import sys
from pathlib import Path

if __name__ == '__main__':
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from tools.asset_pipeline.scene_clips import extract
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    args = parser.parse_args()
    print('Prepared', extract(args.game, args.assets, args.work), 'Trick Guide clips and the set')
