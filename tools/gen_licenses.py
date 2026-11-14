"""Write the Extras > Licenses text (doc 31 "Extras: Licenses"): this project's license and every crate the game
links, from `cargo metadata` (normal dependencies reachable from skate-game; build and dev dependencies left out).

Run after changing dependencies:  python tools/gen_licenses.py
Output: crates/skate-game/src/licenses.txt (embedded in the game; one line per entry, a blank line between sections).
"""
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / 'crates' / 'skate-game' / 'src' / 'licenses.txt'


def main():
    meta = json.loads(subprocess.run(
        ['cargo', 'metadata', '--format-version', '1', '--locked', '--offline',
         '--filter-platform', 'x86_64-pc-windows-msvc'],
        cwd=ROOT, check=True, capture_output=True).stdout)
    packages = {p['id']: p for p in meta['packages']}
    nodes = {n['id']: n for n in meta['resolve']['nodes']}
    start = next(p['id'] for p in meta['packages'] if p['name'] == 'skate-game')
    seen, todo = set(), [start]
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]['deps']:
            if any(k['kind'] is None for k in dep['dep_kinds']):
                todo.append(dep['pkg'])
    workspace = set(meta['workspace_members'])
    third = sorted({(packages[p]['name'], packages[p]['version'], packages[p].get('license') or 'see the crate')
                    for p in seen if p not in workspace})
    lines = [
        'Skate 3 Rust engine',
        'This program is free software: GNU General Public License, version 3 (see LICENSE).',
        'Skate 3 is a trademark of Electronic Arts. This project ships no game code or data: the game files come from your own copy.',
        '',
        f'Third-party software ({len(third)} crates, each under its own license):',
    ]
    lines += [f'{name} {version} ({lic})' for name, version, lic in third]
    OUT.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    print(f'wrote {OUT} ({len(third)} crates)')


if __name__ == '__main__':
    sys.exit(main())
