"""Extract one district's collision triangles for analysis (does not touch the install).

Usage (repo root): py -3.13 tools/collision-inspect/map_collision.py MegaPark [--disc DIR] [--work DIR]
Extracts world<District>.big from the disc into <work>/<District>/raw if needed, then writes
<work>/<District>/collision.npy (N x 3 x 3 float64, Y-up metres) and
prints the bounds and the spawn the current map_writer rule would choose.
Parks take ~1 s; city districts take minutes and a lot of memory.
"""
import argparse
import os
import sys
from pathlib import Path
import numpy as np

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.asset_pipeline import install
from tools.asset_pipeline.map_writer import SpawnSelector
sys.path.insert(0, str(install.TOOLS / 'vendor/university/tools/vanilla_map_extraction/tools'))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import unsigned_collision  # noqa: F401  (unsigned vertex deltas, as the game reads them)
from prepare_hawaiian_dream import prepare
from prepare_university import EXCLUDED_NORMAL_TEXTURE_IDS

parser = argparse.ArgumentParser(description="Extract one district's collision triangles for analysis.")
parser.add_argument('district', nargs='?', default='MegaPark', help='district name without DIST_ (default: %(default)s)')
parser.add_argument('--disc', type=Path, default=Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')),
                    help='extracted disc root, the folder holding data/ (default: %(default)s)')
parser.add_argument('--work', type=Path, default=REPO / '.local/collision', help='work folder (default: %(default)s)')
args = parser.parse_args()
name = args.district
district = 'DIST_' + name
archive = args.disc / 'data/content' / f'world{district}.big'
work = args.work / name
if not (work / 'raw').is_dir():
    install.extract(archive, work / 'raw')
stream = work / 'raw/data/content/world/stream' / district
spawn = SpawnSelector(district)
triangles = []

def consume(meshes):
    spawn.consider(meshes)
    for mesh in meshes:
        triangles.extend((t.a, t.b, t.c) for t in mesh.triangles)

prepare(stream_directory=stream, output_root=work / 'intermediate', utt_root=install.TOOLS / 'vendor/utt',
        district_name=district, map_name=name, package_name='analysis', cache_format='skate3-rust-map-v1',
        texture_stream_names=('Tex',) if any(stream.glob('cTex_*.xsf')) else (),
        excluded_normal_texture_ids=EXCLUDED_NORMAL_TEXTURE_IDS, raw_texture_cache=True,
        collision_consumer=consume, write_render_sources=False)
t = np.asarray(triangles, dtype=np.float64)
np.save(work / 'collision.npy', t)
points = t.reshape(-1, 3)
print('triangles', len(t))
print('bounds min', points.min(0).round(1), 'max', points.max(0).round(1))
print('spawn (current rule)', np.round(spawn.result(name), 3))
