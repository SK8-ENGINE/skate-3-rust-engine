"""Each map's Skate 3 name for the map lists (user 2026-10-20: retail names for every map, the file stem stays the id).

The rule is ours (no retail screen lists maps by stream): the `map_category` row keyed like the map's `world` row
(the cities: `dist_university` -> ID_DISTRICT_UNIVERSITY; DLC rows drop the `dist_`), else the world row's own name field (the parks and the DLC
worlds: ID_WORLD_MALOOFMONEYCUP_NYC), resolved in the disc's language table and the DLC packs' own (research b416).
A map without a resolvable name keeps its label.
"""
from pathlib import Path

from .environment import Collections, key_hash

WORLD_NAME = 'Hash_095E1D9778F0C6F4'
CATEGORY_LABEL = 'Hash_465698DD3CC73CA2'


def _own_text(row, field):
    value = row['fields'].get(field)
    return value['data'] if value and value.get('data') and not value['data'].startswith('#') else None


def names(converted: dict, dlc_vaults, language: dict[str, str]) -> dict[str, dict]:
    """{map label: {'name_id', 'name'}} for every world row whose stream gives a map label."""
    from .dlc import merged
    collections = Collections(merged(converted, list(dlc_vaults)))
    categories = {key_hash(row['key']): row for (cls, _), row in collections.rows.items() if cls == key_hash('map_category')}
    out = {}
    for (cls, key), row in collections.rows.items():
        if cls != key_hash('world'):
            continue
        fields, _ = collections.resolve('world', key)
        stream = fields.get(key_hash('WorldStream'))
        stream = stream['data'] if stream else None
        if not stream:
            continue
        label = stream[5:] if stream.lower().startswith('dist_') else stream
        # The stream's own row only (parks list full / empty / tutorial rows too).
        own = row['key'].casefold() in (stream.casefold(), ('dist_' + label).casefold())
        if not own and label in out:
            continue
        # Keyed like the world row (`dist_university`) or without its `dist_` (the DLC rows: `sanitariumdlc`, whose
        # ID_LOCATION_SANITARIUMDLC "The Sanatorium" is what Skate 3's challenge map shows).
        category = categories.get(key) or (categories.get(key_hash(row['key'][5:]))
                                           if row['key'].lower().startswith('dist_') else None)
        name_id = (category and _own_text(category, CATEGORY_LABEL)) or _own_text(row, WORLD_NAME)
        text = language.get(name_id) if name_id else None
        # The menu fonts draw glyphs in byte positions (the school's name starts with one): such a text, like a
        # missing one, falls back to the label in words.
        if text and any(ord(c) > 0x7e and not c.isalpha() for c in text):
            text = None
        if own or label not in out:
            out[label] = {'name_id': name_id if text else None, 'name': text.strip() if text else words(label)}
    return out


def words(label: str) -> str:
    """`MaloofMoneyCup` -> `Maloof Money Cup`, `DLC_DW_MegaCompund` -> `DLC DW Mega Compund` (ours, for names retail
    gives no text for)."""
    import re
    return re.sub(r'(?<=[a-z])(?=[A-Z])', ' ', label.replace('_', ' ')).strip()


def disc_language(game_root: Path, work: Path) -> dict[str, str]:
    """The disc's English text (the pair the menu export reads, from its UI archives)."""
    from tools.owned_game.big import BigArchive
    import sys
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from vendor.skate3_ui.language import pair_language_tables
    from vendor.skate3_ui.project import CORE_ARCHIVES
    wanted = {'data/fe/languages/labels/language_labels_global_skate3ng.bin': 'labels.bin',
              'data/fe/languages/english/language_english_global_skate3ng.bin': 'english.bin'}
    work.mkdir(parents=True, exist_ok=True)
    for name in CORE_ARCHIVES:
        archive = BigArchive(Path(game_root) / 'data/big' / name)
        for entry in archive.entries:
            target = wanted.get(entry.path.replace('\\', '/').lower())
            if target:
                (work / target).write_bytes(archive.read(entry))
    pair = pair_language_tables(work / 'labels.bin', work / 'english.bin')
    return {row['label'].strip(): row['value'] for row in pair['entries'] if row['label']}
