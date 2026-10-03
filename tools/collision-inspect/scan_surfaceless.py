"""For every district: find collision meshes with no surface ids and describe them.

Usage (repo root): py -3.13 tools/collision-inspect/scan_surfaceless.py [District ...] [--disc DIR] [--work DIR]
Extracts each district into <work>/<District>/ (default .local/collision) if needed (cities take minutes),
then prints per-mesh counts: surfaced / mixed (some units lack surface ids) / surfaceless.
"""
import argparse, collections, json, os, sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.asset_pipeline import install
TOOLS = install.TOOLS / 'vendor/university/tools/vanilla_map_extraction/tools'
sys.path.insert(0, str(TOOLS))
from prepare_hawaiian_dream import prepare
from prepare_university import EXCLUDED_NORMAL_TEXTURE_IDS
sys.path.insert(0, str(Path(__file__).resolve().parent))
import unsigned_collision  # noqa: F401  (unsigned vertex deltas, as the game reads them)
from retail_collision_mesh import decode_rx2_clustered_meshes

parser = argparse.ArgumentParser(description='Find collision meshes with no surface ids and describe them.')
parser.add_argument('districts', nargs='*', help='district names without DIST_ (default: all ten)')
parser.add_argument('--disc', type=Path, default=Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')),
                    help='extracted disc root, the folder holding data/ (default: %(default)s)')
parser.add_argument('--work', type=Path, default=REPO / '.local/collision', help='work folder (default: %(default)s)')
args = parser.parse_args()
names = args.districts or ['MegaPark', 'IndustrialSkatePark', 'StartPark', 'BlackBoxPark', 'DownTownSkatePark',
                         'MaloofMoneyCup', 'SkateSchool', 'University', 'Industrial', 'DownTown']
for name in names:
    district = 'DIST_' + name
    work = args.work / name
    if not (work / 'raw').is_dir():
        install.extract(args.disc / f'data/content/world{district}.big', work / 'raw')
    stream = work / 'raw/data/content/world/stream' / district
    out = work / 'scan'
    if not (out / 'manifest.json').is_file():
        prepare(stream_directory=stream, output_root=out, utt_root=install.TOOLS / 'vendor/utt',
                district_name=district, map_name=name, package_name='analysis', cache_format='skate3-rust-map-v1',
                texture_stream_names=('Tex',) if any(stream.glob('cTex_*.xsf')) else (),
                excluded_normal_texture_ids=EXCLUDED_NORMAL_TEXTURE_IDS, raw_texture_cache=True,
                collision_consumer=lambda meshes: None, write_render_sources=False)
    manifest = json.loads((out / 'manifest.json').read_text())
    total = collections.Counter()
    special = []
    for entry in manifest['simulation_assets']:
        if not entry.get('collision_meshes'):
            continue
        for mesh in decode_rx2_clustered_meshes((out / entry['rx2']).read_bytes()):
            flags = {bool(t.unit_flags & 0x80) for t in mesh.triangles}
            kind = 'surfaced' if flags == {True} else 'surfaceless' if flags == {False} else 'mixed'
            total[kind] += 1
            if kind != 'surfaced':
                size = [round(mesh.bounds_max[k] - mesh.bounds_min[k], 1) for k in range(3)]
                lo = [round(v, 2) for v in mesh.bounds_min]
                hi = [round(v, 2) for v in mesh.bounds_max]
                special.append(f"{kind} {entry['stream_file']} tris={len(mesh.triangles)} size={size} "
                               f"min={lo} max={hi}")
    print(f'== {name}: {dict(total)}', flush=True)
    for line in special:
        print('   ', line, flush=True)
