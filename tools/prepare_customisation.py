"""Prepare original resident character choices during setup, without a game."""
import argparse
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.asset_pipeline.customisation_catalog import prepare as catalog
from tools.asset_pipeline.customisation_library import prepare as library
from tools.asset_pipeline.customisation_profiles import generate


def prepare(game, assets):
    cache = assets / "private/customisation"
    catalog(game, cache)
    result = library({"game_root": str(game), "assets": str(assets), "library_index": "library-v3.json"})
    if result["errors"]:
        raise ValueError(f"Original character library is incomplete: {result['errors']}")
    native = json.loads((cache / "native.json").read_text(encoding="utf-8"))
    (cache / "extra-menu.json").write_text(json.dumps(generate(native), indent=2), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, required=True)
    parser.add_argument("--assets", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.game, args.assets)
