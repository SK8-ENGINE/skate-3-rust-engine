"""Extract the built-in PCA ocean/water animation (assets/private/ocean-pca.json).

`convert` is what setup uses: the game executable unpacks the owned
default.xex and locates the table in any build (skate_data::ocean_pca,
`skate3rust --extract-ocean-pca`). Without this file every water and ocean
material renders as a static fallback.

`extract` is the original TU3-only path from an already mapped image.
Addresses are corroborated by cPCAWaterAnimationData::Init (827905B0) and
its update routine (82790858). No game content is embedded in this tool.
"""
import hashlib
import json
import math
import struct
from pathlib import Path


def extract(image, output, base=0x82000000):
    data = Path(image).read_bytes()
    identity = hashlib.sha256(data).hexdigest()
    if identity != 'f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4':
        raise ValueError('Unsupported mapped executable build for PCA table extraction')
    frames = []
    for frame in range(30):
        mean = struct.unpack_from('>3f', data, 0x830118D8-base+frame*12)
        weights = struct.unpack_from('>24f', data, 0x83011A40-base+frame*96)
        if not all(math.isfinite(v) for v in (*mean, *weights)):
            raise ValueError('Non-finite ocean PCA table')
        # Shader model order: mean X/Z/Y and weight pairs R/B/G.
        rows = [[mean[0]/255., mean[2]/255., mean[1]/255., 0.]]
        for start in (0, 4, 16, 20, 8, 12):
            rows.append([v/255. for v in weights[start:start+4]])
        frames.append(rows)
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(dict(source_sha256=identity, hz=30., frames=frames)), encoding='utf-8')
    return len(frames)


def convert(game_exe, xex, assets, log=None):
    """Writes assets/private/ocean-pca.json from the owned default.xex."""
    import subprocess
    from .install import spawn
    output = Path(assets) / 'private/ocean-pca.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    with spawn([str(game_exe), '--extract-ocean-pca', str(xex), str(output)],
               stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
        text = process.stdout.read()  # spawn() opens text-mode pipes
        code = process.wait()
    if log is not None:
        log.write(text)
        log.flush()
    if code or 'OCEAN_PCA_READY' not in text:
        output.unlink(missing_ok=True)
        raise RuntimeError('Ocean animation extraction failed: ' + (text.strip().splitlines() or ['no output'])[-1])
    frames = json.loads(output.read_text(encoding='utf-8'))['frames']
    if len(frames) != 30:
        raise ValueError('Ocean animation table must have 30 frames')
    return len(frames)


if __name__ == '__main__':
    import argparse
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--image', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    print('Extracted', extract(a.image, a.output), 'ocean PCA frames')
