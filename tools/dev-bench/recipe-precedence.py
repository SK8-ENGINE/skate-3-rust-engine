"""Recipe precedence check (doc 39 "NOT RETAIL YET", b425 open): lists every static / dynamic recipe piece id whose
row differs between the disc's and the DLC packs' recipe XMLs. No conflicts = the load order cannot change the outcome.

usage: python recipe-precedence.py <worktree> <disc folder> <dlc folder> <work folder>
"""
import sys
from pathlib import Path

tree, disc, dlc, work = map(Path, sys.argv[1:5])
sys.path.insert(0, str(tree))
from tools.owned_game.big import BigArchive  # noqa: E402
from tools.asset_pipeline import dlc as dlc_packages  # noqa: E402
from tools.asset_pipeline.park_layout import recipe, STATIC_RECIPE, DYNAMIC_RECIPE  # noqa: E402

archives = [disc / 'data/big/miscload.big', disc / 'data/big/miscboot.big', disc / 'data/content/parkassets.big']
archives += dlc_packages.extract_bigs(dlc, work, lambda paths: any(p.lower().startswith('data/content/recipe/')
                                                                   for p in paths))
for kind in (STATIC_RECIPE, DYNAMIC_RECIPE):
    rows = {}  # id -> [(source, row)]
    for big in archives:
        archive = BigArchive(big)
        for entry in sorted(archive.entries, key=lambda e: e.path.lower()):
            path = entry.path.replace('\\', '/').lower()
            if path.endswith('.xml') and path.startswith(kind):
                source = f'{big.parent.name}/{big.name}:{Path(path).name}'
                found = recipe(archive.read(entry))
                print(f'{kind} {source}: {len(found)} ids')
                for key, row in found.items():
                    rows.setdefault(key, []).append((source, row))
    shared = {k: v for k, v in rows.items() if len(v) > 1}
    conflicts = {k: v for k, v in shared.items() if any(row != v[0][1] for _, row in v[1:])}
    print(f'{kind}: {len(rows)} ids, {len(shared)} in more than one file, {len(conflicts)} differ')
    for key, entries in sorted(conflicts.items()):
        print(f'  {key}')
        for source, row in entries:
            print(f'    {source}: {row}')
