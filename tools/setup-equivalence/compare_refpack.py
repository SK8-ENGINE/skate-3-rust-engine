"""Prove the native RefPack DLL decodes exactly like the pure-Python decoder.

Usage (repo root, after scripts\\Build.ps1 built target/native/refpack.dll):
    py -3.13 tools/setup-equivalence/compare_refpack.py [--streams] [--disc DIR] [--work DIR]

Decodes every entry of the character archives (createacharacter.big, marquee.big — what
the customiser reads) with the Python decoder (fast_refpack._library = None) and with the
DLL, and compares bytes. --streams also compares every district stream asset (map stage) of the
districts extracted under --work (default .local/collision, see tools/collision-inspect).
--disc is the extracted disc root (the folder holding data/; default $SKATE3_DISC or .local/skate3-disc).
Exit 1 on any difference.
"""
import argparse, os, sys, time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
sys.path.insert(0, str(REPO / 'tools/vendor/university/tools/vanilla_map_extraction/tools'))
from tools.asset_pipeline import fast_refpack
from tools.owned_game.big import BigArchive

parser = argparse.ArgumentParser(description='Prove the native RefPack DLL decodes exactly like the Python decoder.')
parser.add_argument('--streams', action='store_true', help='also compare every extracted district stream asset')
parser.add_argument('--disc', type=Path, default=Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')),
                    help='extracted disc root (default: %(default)s)')
parser.add_argument('--work', type=Path, default=REPO / '.local/collision', help='extracted districts (default: %(default)s)')
args = parser.parse_args()
native = fast_refpack._library
if native is None:
    sys.exit('target/native/refpack.dll not loaded: run scripts\\Build.ps1 first')
disc = args.disc / 'data/content'


def decode_all(read):
    fast_refpack._library = None
    start = time.perf_counter(); python = read(); t_python = time.perf_counter() - start
    fast_refpack._library = native
    start = time.perf_counter(); fast = read(); t_native = time.perf_counter() - start
    return python, fast, t_python, t_native


failed = False
for name in ('createacharacter.big', 'marquee.big'):
    archive = BigArchive(disc / name)
    python, fast, t_python, t_native = decode_all(lambda: [archive.read(e) for e in archive.entries])
    compressed = sum(1 for e in archive.entries if e.compression)
    same = python == fast
    failed |= not same
    print(f'{name:22} entries {len(archive.entries):5} (compressed {compressed:5})  python {t_python:6.1f}s  native {t_native:5.1f}s  {"IDENTICAL" if same else "DIFFERENT"}', flush=True)

if args.streams:
    import skate3_streams
    for district in sorted(args.work.iterdir()):
        stream = district / 'raw/data/content/world/stream' / f'DIST_{district.name}'
        for kind in ('Pres', 'Sim', 'Tex'):
            if not (stream / f'DIST_{district.name}_{kind}.xst').is_file():
                continue
            python, fast, t_python, t_native = decode_all(
                lambda: [a.data for a in skate3_streams.load_district_stream(stream, kind, f'DIST_{district.name}')])
            same = python == fast
            failed |= not same
            print(f'{district.name:20} {kind:4} assets {len(python):5}  python {t_python:6.1f}s  native {t_native:5.1f}s  {"IDENTICAL" if same else "DIFFERENT"}', flush=True)

print('ALL IDENTICAL' if not failed else 'DIFFERENCES FOUND')
sys.exit(1 if failed else 0)
