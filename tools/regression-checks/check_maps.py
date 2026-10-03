"""Validate every installed map in ONE game process and compare against baselines.

Usage (repo root, after building/staging bin\\skate3rust.exe):
    py -3.13 tools/regression-checks/check_maps.py --update   # first, on a known-good build: record the baseline
    py -3.13 tools/regression-checks/check_maps.py            # later: compare against it

Uses `skate3rust --validate-maps` (crates/skate-game/src/map_validation.rs): TEST_WORLD plus
each map. Fails if a map is not ok, has warnings (spawn support / startup / invisible floor),
or its loaded collision triangle count differs from the baseline
(default .local/regression/collision_baseline.json; --baseline PATH, --exe PATH).
Falls back to one `--check-assets` launch per map if the exe has no --validate-maps.
"""
import argparse
import json
import re
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BASELINE = REPO / '.local/regression/collision_baseline.json'


def validate(exe, maps):
    process = subprocess.Popen([str(exe), '--assets', str(REPO / 'assets'), '--validate-maps'],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                               text=True, cwd=REPO)
    if not process.stdout.readline().startswith('SKATE_VALIDATOR_READY'):
        process.kill()
        return None
    results = {}
    for request in ['TEST_WORLD', *map(str, maps)]:
        process.stdin.write(request + '\n'); process.stdin.flush()
        line = process.stdout.readline()
        if not line.startswith('SKATE_MAP_CHECK '):
            raise RuntimeError(f'validator stopped at {request}')
        result = json.loads(line[len('SKATE_MAP_CHECK '):])
        results['TEST_WORLD' if request == 'TEST_WORLD' else Path(request).stem] = result
    process.stdin.close(); process.wait()
    return results


def check_assets(exe, maps):
    results = {}
    for path in maps:
        run = subprocess.run([str(exe), '--assets', str(REPO / 'assets'), '--map', str(path), '--check-assets'],
                             capture_output=True, text=True, errors='replace', cwd=REPO)
        match = re.search(r'SKATE_RWCM_READY triangles=(\d+)', run.stdout + run.stderr)
        results[path.stem] = {'ok': run.returncode == 0 and 'SKATE_ASSETS_READY' in run.stdout + run.stderr,
                              'errors': [], 'warnings': [], 'collision_triangles': int(match.group(1)) if match else None}
    return results


def main():
    parser = argparse.ArgumentParser(description='Validate every installed map and compare collision triangle counts '
                                                 'with a baseline.')
    parser.add_argument('--update', action='store_true', help='write the current values as the new baseline')
    parser.add_argument('--baseline', type=Path, default=BASELINE, help='baseline JSON (default: %(default)s)')
    parser.add_argument('--exe', type=Path, default=REPO / 'bin/skate3rust.exe', help='game exe (default: %(default)s)')
    args = parser.parse_args()
    marker = json.loads((REPO / 'data/installation.json').read_text())
    maps = sorted((REPO / 'data' / marker['directory'] / 'maps').glob('*.skate'))
    exe = args.exe
    results = validate(exe, maps)
    if results is None:
        print('(no --validate-maps in this exe: using --check-assets per map, no spawn checks)')
        results = check_assets(exe, maps)
    current = {name: r['collision_triangles'] for name, r in results.items() if name != 'TEST_WORLD'}
    failed = False
    for name, r in results.items():
        if not r['ok'] or r['warnings']:
            failed = True
            print(f"{name:20} NOT OK errors={r['errors']} warnings={r['warnings']}")
    if args.update:
        args.baseline.parent.mkdir(parents=True, exist_ok=True)
        args.baseline.write_text(json.dumps(current, indent=2) + '\n')
        print(f'baseline updated with {len(current)} maps from {marker["directory"]}')
        return 1 if failed else 0
    if not args.baseline.is_file():
        print(f'no baseline at {args.baseline}: run once with --update on a known-good build')
        return 1
    baseline = json.loads(args.baseline.read_text())
    for name in sorted(set(baseline) | set(current)):
        want, got = baseline.get(name), current.get(name)
        status = 'ok' if want == got else f'CHANGED (baseline {want}, delta {None if None in (want, got) else got - want})'
        failed |= want != got
        seconds = results.get(name, {}).get('seconds')
        print(f'{name:20} {got}  {status}' + (f'  ({seconds:.2f}s)' if seconds is not None else ''))
    return 1 if failed else 0


if __name__ == '__main__':
    raise SystemExit(main())
