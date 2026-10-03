"""Compare every installed map's baked spawn with the known-good baseline.

Usage (from the repo root):
    py -3.13 tools/regression-checks/check_spawns.py --update   # first, on a known-good install: record the baseline
    py -3.13 tools/regression-checks/check_spawns.py            # later: compare against it

The installation is read from data/installation.json; the baseline defaults to
.local/regression/spawn_baseline.json (--baseline PATH). Exit code 1 if any map differs.
"""
import argparse
import json
import struct
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / '.local/regression/spawn_baseline.json'
TOLERANCE = 0.01


def read_spawn(path):
    # SKATE14 header: magic, u32, length-prefixed name, then 3 x f32 spawn.
    with open(path, 'rb') as f:
        magic = f.read(8)
        if not magic.startswith(b'SKATE'):
            raise ValueError(f'{path.name}: not a .skate file')
        f.read(4)
        (length,) = struct.unpack('<I', f.read(4))
        name = f.read(length).decode()
        return name, list(struct.unpack('<3f', f.read(12)))


def main():
    parser = argparse.ArgumentParser(description="Compare every installed map's baked spawn with a baseline.")
    parser.add_argument('--update', action='store_true', help='write the current values as the new baseline')
    parser.add_argument('--baseline', type=Path, default=BASELINE, help='baseline JSON (default: %(default)s)')
    args = parser.parse_args()
    marker = json.loads((REPO / 'data/installation.json').read_text())
    maps = REPO / 'data' / marker['directory'] / 'maps'
    current = dict(read_spawn(p) for p in sorted(maps.glob('*.skate')))
    if args.update:
        args.baseline.parent.mkdir(parents=True, exist_ok=True)
        args.baseline.write_text(json.dumps({k: [round(v, 4) for v in s] for k, s in current.items()}, indent=2) + '\n')
        print(f'baseline updated with {len(current)} maps from {marker["directory"]}')
        return 0
    if not args.baseline.is_file():
        print(f'no baseline at {args.baseline}: run once with --update on a known-good install')
        return 1
    baseline = json.loads(args.baseline.read_text())
    failed = False
    for name in sorted(set(baseline) | set(current)):
        want, got = baseline.get(name), current.get(name)
        if want is None or got is None:
            status = 'NEW MAP' if want is None else 'MISSING'
            failed = True
        elif all(abs(a - b) <= TOLERANCE for a, b in zip(want, got)):
            status = 'ok'
        else:
            status = f'CHANGED (baseline {want})'
            failed = True
        print(f'{name:20} {[round(v, 2) for v in got] if got else "-"}  {status}')
    return 1 if failed else 0


if __name__ == '__main__':
    raise SystemExit(main())
