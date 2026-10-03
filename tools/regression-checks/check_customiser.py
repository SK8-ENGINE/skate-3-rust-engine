"""Snapshot / compare the character customiser outputs of the current installation.

Usage (repo root):
    py -3.13 tools/regression-checks/check_customiser.py --save   # snapshot the current install
    py -3.13 tools/regression-checks/check_customiser.py          # compare current install to snapshot

Snapshot (default .local/customiser-baseline/, --snapshot DIR): every stage receipt (*-complete.json: relative path ->
size + sha256 of each output file), every JSON file of the set, and the set id. JSON files may embed
the random set id in paths, so a JSON file whose hash differs is compared again with the set id
replaced. Exit 1 on any difference. Use after changing anything the customiser could touch
(stage scheduling, shared parsers, PIL/numpy versions, ...).
"""
import argparse
import json
import shutil
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SNAPSHOT = REPO / '.local/customiser-baseline'


def current_set():
    marker = json.loads((REPO / 'data/installation.json').read_text())
    custom = REPO / 'data' / marker['directory'] / 'assets/private/customisation'
    set_id = json.loads((custom / 'current.json').read_text())['set']
    return set_id, custom / 'sets' / set_id


def receipts(directory):
    files = {}
    for path in sorted(directory.glob('*-complete.json')):
        files.update(json.loads(path.read_text())['files'])
    return files


def save():
    set_id, directory = current_set()
    if SNAPSHOT.exists():
        shutil.rmtree(SNAPSHOT)
    (SNAPSHOT / 'json').mkdir(parents=True)
    for path in directory.glob('*-complete.json'):
        shutil.copy2(path, SNAPSHOT / path.name)
    for path in directory.rglob('*.json'):
        target = SNAPSHOT / 'json' / path.relative_to(directory)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
    (SNAPSHOT / 'SET_ID').write_text(set_id)
    print(f'snapshot of set {set_id}: {len(receipts(directory))} receipted files')
    return 0


def compare():
    set_id, directory = current_set()
    if not (SNAPSHOT / 'SET_ID').is_file():
        print(f'no snapshot in {SNAPSHOT}: run once with --save on a known-good install')
        return 1
    old_id = (SNAPSHOT / 'SET_ID').read_text().strip()
    old, new = receipts(SNAPSHOT), receipts(directory)
    problems = []
    for name in sorted(set(old) | set(new)):
        if name not in new or name not in old:
            problems.append(f'{"missing" if name not in new else "new"}: {name}')
        elif old[name]['sha256'] != new[name]['sha256']:
            saved = SNAPSHOT / 'json' / name
            if name.endswith('.json') and saved.is_file():
                if saved.read_text(encoding='utf-8').replace(old_id, set_id) == (directory / name).read_text(encoding='utf-8'):
                    continue  # differs only by the embedded set id
            problems.append(f'changed: {name}')
    print(f'compared {len(old)} snapshot files with {len(new)} current files (set {old_id} -> {set_id})')
    for problem in problems[:40]:
        print('  ' + problem)
    print('IDENTICAL' if not problems else f'{len(problems)} DIFFERENCES')
    return 1 if problems else 0


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Snapshot / compare the character customiser outputs of the current installation.')
    parser.add_argument('--save', action='store_true', help='snapshot the current install (else compare with the snapshot)')
    parser.add_argument('--snapshot', type=Path, default=SNAPSHOT, help='snapshot folder (default: %(default)s)')
    args = parser.parse_args()
    SNAPSHOT = args.snapshot
    raise SystemExit(save() if args.save else compare())
