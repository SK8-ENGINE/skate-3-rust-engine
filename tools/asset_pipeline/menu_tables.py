"""Extract the retail front-end menu tables (assets/private/menu-tables.json).

`convert` is what setup uses: the game executable unpacks the owned
default.xex and locates the tables in any build (skate_data::menu_tables,
`skate3rust --extract-menu-tables`): Game Settings rows and screens, pause-menu
(crossbar) categories and items, the Apt key table, the per-mode menus and the
Trick Guide's category order and slot name tables, and the career stats rows.
Without this file the game's menu registry starts empty (mods can still add
entries). No game content is embedded in this tool.
"""
import json
from pathlib import Path


def convert(game_exe, xex, assets, log=None):
    """Writes assets/private/menu-tables.json from the owned default.xex."""
    import subprocess
    from .install import spawn
    output = Path(assets) / 'private/menu-tables.json'
    output.parent.mkdir(parents=True, exist_ok=True)
    with spawn([str(game_exe), '--extract-menu-tables', str(xex), str(output)],
               stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
        text = process.stdout.read()  # spawn() opens text-mode pipes
        code = process.wait()
    if log is not None:
        log.write(text)
        log.flush()
    if code or 'MENU_TABLES_READY' not in text:
        output.unlink(missing_ok=True)
        raise RuntimeError('Menu table extraction failed: ' + (text.strip().splitlines() or ['no output'])[-1])
    tables = json.loads(output.read_text(encoding='utf-8'))
    if tables.get('schema') != 1 or not tables.get('items') or not tables.get('modes') or not tables.get('templates') or not tables.get('screens') or not tables.get('settings_titles') or not tables.get('sub_options') or not tables.get('trick_guide', {}).get('categories') or not tables.get('career_stats'):
        output.unlink(missing_ok=True)
        raise ValueError('Menu tables are incomplete')
    return len(tables['items'])
