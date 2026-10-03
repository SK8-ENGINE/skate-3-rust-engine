"""Capture a district's collision triangles with surface ids, unit flags and mesh flags.

Usage (repo root, after map_collision.py extracted the district):
    py -3.13 tools/collision-inspect/collision_attrs.py SkateSchool [--work DIR]
Writes <work>/<District>/collision_attrs.npy (default work .local/collision) with rows:
    9 vertex floats, surface id, unit flags (0x80 = has surface id), mesh flags, mesh index.
"""
import argparse, sys
from pathlib import Path
import numpy as np

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.asset_pipeline import install
sys.path.insert(0, str(install.TOOLS / 'vendor/university/tools/vanilla_map_extraction/tools'))
from prepare_hawaiian_dream import prepare
from prepare_university import EXCLUDED_NORMAL_TEXTURE_IDS

parser = argparse.ArgumentParser(description="Capture a district's collision triangles with surface ids and flags.")
parser.add_argument('district', nargs='?', default='SkateSchool', help='district name without DIST_ (default: %(default)s)')
parser.add_argument('--work', type=Path, default=REPO / '.local/collision', help='work folder (default: %(default)s)')
args = parser.parse_args()
name = args.district
district = 'DIST_' + name
work = args.work / name
stream = work / 'raw/data/content/world/stream' / district
rows = []
mesh_index = [0]
def consume(meshes):
    for mesh in meshes:
        for t in mesh.triangles:
            rows.append((*t.a, *t.b, *t.c, t.surface, t.unit_flags, mesh.mesh_flags, mesh_index[0]))
        mesh_index[0] += 1
prepare(stream_directory=stream, output_root=work / 'intermediate2', utt_root=install.TOOLS / 'vendor/utt',
        district_name=district, map_name=name, package_name='analysis', cache_format='skate3-rust-map-v1',
        texture_stream_names=('Tex',) if any(stream.glob('cTex_*.xsf')) else (),
        excluded_normal_texture_ids=EXCLUDED_NORMAL_TEXTURE_IDS, raw_texture_cache=True,
        collision_consumer=consume, write_render_sources=False)
a = np.asarray(rows, dtype=np.float64)
np.save(work / 'collision_attrs.npy', a)
print('triangles', len(a), 'meshes', mesh_index[0])
