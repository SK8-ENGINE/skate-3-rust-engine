"""Incrementally prepare original HUD assets from the user's extracted game.

No executable/game launch. Output contains copyrighted assets and stays private.
The vendored preview.18 UI extractor supplies the original decoding pipeline.
"""
from pathlib import Path
import argparse
import hashlib
import json
import sys

from vendor.skate3_ui.project import extract_project
from vendor.skate3_ui.scene_graph import AssetCache, SceneFlattener
from vendor.skate3_ui.actions import Actions


def prepare(game: Path, output: Path):
    extract_project(game, output, prefixes=("data/fe/source/screens/hud2/",
                    "data/fe/source/controls/"), update=True)
    cache = AssetCache(output)
    name = "data/fe/source/screens/hud2/trickdisplay2"
    bundle = cache.load_bundle(name)
    apt_path = output / "raw" / (name + ".apt")
    const_path = apt_path.with_suffix('.const')
    actions = Actions(apt_path.read_bytes(), const_path.read_bytes())
    blocks = {}
    for c in bundle['characters'].values():
        for f in c.get('frames', []):
            for control in f['controls']:
                if control['type_name'] in ('do_action', 'do_init_action'):
                    offset = control.get('actions_offset', 0)
                    if offset:
                        blocks[str(offset)] = actions.stream(offset)
    shapes = {}
    fonts = {}
    for c in bundle['characters'].values():
        if c['type_name'] == 'shape':
            scene = SceneFlattener(cache, lambda *_: {}).flatten(name, c['id'])
            if scene['unresolved']:
                raise ValueError(f"Unresolved original HUD shape: {scene['unresolved']}")
            shapes[str(c['id'])] = scene['primitives']
        elif c['type_name'] == 'font':
            family = c['font']['name']
            asset = cache.font_asset(family)
            # Preserve this explicit source limitation. Preview.18 inferred a
            # metric alias in HighlightText; that is not proof of retail HUD
            # glyph/raster substitution. Never silently substitute a font.
            fonts[family] = asset
    result = {
        'format': 'skate3-scoring-hud', 'version': 1,
        'source': {'bundle': name,
                   'apt_sha256': hashlib.sha256(apt_path.read_bytes()).hexdigest(),
                   'const_sha256': hashlib.sha256(const_path.read_bytes()).hexdigest()},
        'characters': list(bundle['characters'].values()),
        'shapes': shapes, 'fonts': fonts, 'actions': blocks,
        'unresolved_fonts': [family for family, asset in fonts.items() if asset is None],
    }
    target = output / 'runtime' / 'trickdisplay.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(result, separators=(',', ':')) + '\n', encoding='utf-8')
    print(f"Prepared {len(shapes)} original HUD shapes and {len(blocks)} action blocks: {target}")
    if result['unresolved_fonts']:
        print('Unresolved authored font families: ' + ', '.join(result['unresolved_fonts']))
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--game', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    prepare(args.game.resolve(), args.output.resolve())
