"""Prove a changed stream loader returns exactly what a reference copy returns.

Usage (repo root):
    py -3.13 tools/setup-equivalence/compare_streams.py <reference skate3_streams.py> [District ...] [--work DIR]
Get a reference with: git show <commit>:tools/vendor/university/tools/vanilla_map_extraction/tools/skate3_streams.py > .local/ref.py
Needs <work>/<District>/raw (default work .local/collision) from tools/collision-inspect/map_collision.py
or scan_surfaceless.py.

For every district and stream (Pres, Sim, Tex): load with the reference module and the current one, and
compare every asset's record, source path, source offset, stored size and decoded bytes. Also times both.
Exit 1 on the first difference.
"""
import argparse, importlib.util, sys, time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
TOOLS = REPO / 'tools/vendor/university/tools/vanilla_map_extraction/tools'
sys.path.insert(0, str(TOOLS))
import skate3_streams as new

parser = argparse.ArgumentParser(description='Prove a changed stream loader returns exactly what a reference copy returns.')
parser.add_argument('reference', type=Path, help='reference copy of skate3_streams.py')
parser.add_argument('districts', nargs='*', help='district names without DIST_ (default: all ten)')
parser.add_argument('--work', type=Path, default=REPO / '.local/collision', help='extracted districts (default: %(default)s)')
args = parser.parse_args()
spec = importlib.util.spec_from_file_location('skate3_streams_original', args.reference)
old = importlib.util.module_from_spec(spec); sys.modules[spec.name] = old; spec.loader.exec_module(old)
import dataclasses

names = args.districts or ['MegaPark', 'StartPark', 'BlackBoxPark', 'IndustrialSkatePark', 'DownTownSkatePark',
                         'MaloofMoneyCup', 'SkateSchool', 'Industrial', 'University', 'DownTown']
total_old = total_new = 0.0
for name in names:
    stream = args.work / name / 'raw/data/content/world/stream' / f'DIST_{name}'
    for kind in ('Pres', 'Sim', 'Tex'):
        if not (stream / f'DIST_{name}_{kind}.xst').is_file():
            continue
        t = time.perf_counter(); a = old.load_district_stream(stream, kind, f'DIST_{name}'); t_old = time.perf_counter() - t
        t = time.perf_counter(); b = new.load_district_stream(stream, kind, f'DIST_{name}'); t_new = time.perf_counter() - t
        total_old += t_old; total_new += t_new
        same = len(a) == len(b) and all(
            (dataclasses.astuple(x.record) == dataclasses.astuple(y.record) and x.source_path == y.source_path and x.source_offset == y.source_offset
             and x.stored_size == y.stored_size and x.data == y.data) for x, y in zip(a, b))
        print(f'{name:20} {kind:4} assets {len(a):5}  old {t_old:7.2f}s  new {t_new:7.2f}s  {"IDENTICAL" if same else "DIFFERENT"}', flush=True)
        if not same:
            sys.exit(1)
print(f'TOTAL old {total_old:.1f}s new {total_new:.1f}s')
