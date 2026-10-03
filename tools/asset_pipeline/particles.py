"""Particle sprites from the owned disc (data/big/miscboot.big particletextures.rx2).

Writes assets/private/particles/<name>.png (steam, dust, pebble, grass, leaf,
fluff, cameraflash, water). The game's water splash uses `water.png`
(crates/skate-game/src/water_splash.rs). Texture names follow the
`<name>.Texture` strings in the resource, which appear in texture order.
No game content is embedded in this tool.
"""
import re
import sys
from pathlib import Path

from tools.owned_game.big import BigArchive

SOURCE = 'data/content/textures/particletextures.rx2'
NAME_PATTERN = re.compile(rb'([A-Za-z0-9_]+)\.Texture')


def texture_names(raw):
    """`<name>.Texture` strings in file order, one per texture."""
    return [m.group(1).decode('ascii').lower() for m in NAME_PATTERN.finditer(raw)]


def convert(game_root, assets):
    vendor = Path(__file__).resolve().parents[1] / 'vendor'
    sys.path.insert(0, str(vendor / 'utt'))
    import rx2_parser
    from PIL import Image

    archive = BigArchive(Path(game_root) / 'data/big/miscboot.big')
    entry = next((e for e in archive.entries if e.path == SOURCE), None)
    if entry is None:
        raise FileNotFoundError(SOURCE)
    raw = archive.read(entry)
    parsed = rx2_parser.parse_rx2(raw)
    names = texture_names(raw)
    if len(names) != len(parsed.textures) or len(set(names)) != len(names):
        raise ValueError(f'Particle texture names do not match textures: {names} vs {len(parsed.textures)}')
    output = Path(assets) / 'private/particles'
    output.mkdir(parents=True, exist_ok=True)
    for name, texture in zip(names, parsed.textures):
        if not texture.rgba:
            raise ValueError(f'Particle texture {name} did not decode')
        Image.frombytes('RGBA', (texture.width, texture.height), texture.rgba).save(output / f'{name}.png')
    return names
