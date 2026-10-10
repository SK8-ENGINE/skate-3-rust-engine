"""Clip names for the living-world animation remaps (peds milestone M2, doc 26).

``tAnimAttributes.anim`` and ``tAnimTakedown.anim`` / ``anim_b`` in ``livingworld_entity_animation``
and ``livingworld_entity_takedown`` are not clip ids: they are byte offsets into the string pool of
``skatercollections.bin`` (e.g. 52075 -> ``NPC_WNDR_WLK_N_0_CYC``) [data, checked against the disc].
The tables export keeps the number (stable key) and this module adds the resolved name next to it
(``anim_name`` / ``anim_b_name``), so the engine's ped animation player can remap logical names to
clips. Nothing else in ``tables.json`` changes.
"""
from __future__ import annotations

from pathlib import Path

ANIM_CLASSES = ('livingworld_entity_animation', 'livingworld_entity_takedown')
NAME_FIELDS = (('anim', 'anim_name'), ('anim_b', 'anim_b_name'))


def string_at(blob: bytes, offset) -> str | None:
    """The NUL-terminated ASCII clip name at ``offset`` (None when it is not a plausible name)."""
    if not isinstance(offset, int) or isinstance(offset, bool) or offset <= 0 or offset >= len(blob):
        return None
    end = blob.find(b'\0', offset, offset + 64)
    if end <= offset:
        return None
    raw = blob[offset:end]
    if not all(48 <= c <= 57 or 65 <= c <= 90 or 97 <= c <= 122 or c == 95 for c in raw):
        return None
    # Offsets point at the start of a string: the byte before is the previous string's NUL.
    if blob[offset - 1] != 0:
        return None
    return raw.decode('ascii')


def _walk(value, blob: bytes) -> int:
    count = 0
    if isinstance(value, list):
        for item in value:
            count += _walk(item, blob)
    elif isinstance(value, dict):
        for raw_key, name_key in NAME_FIELDS:
            if raw_key in value and name_key not in value:
                name = string_at(blob, value[raw_key])
                if name is not None:
                    value[name_key] = name
                    count += 1
        for item in list(value.values()):
            if isinstance(item, (dict, list)):
                count += _walk(item, blob)
    return count


def resolve_anim_names(doc: dict, blob: bytes | None) -> int:
    """Add ``anim_name`` / ``anim_b_name`` to the animation structs of ``doc`` (tables.json content).
    Returns how many names were added (0 when the collections bin is unavailable)."""
    if not blob:
        return 0
    count = 0
    for cls in ANIM_CLASSES:
        for record in doc.get('classes', {}).get(cls, {}).values():
            count += _walk(record.get('fields', {}), blob)
    return count


def collections_bin(game_root: Path, work: Path) -> bytes | None:
    """``skatercollections.bin``: the copy the database conversion staged, else from ``db.big``."""
    staged = Path(work)/'living_world_db'/'database'/'skatercollections.bin'
    if staged.exists():
        return staged.read_bytes()
    loose = Path(game_root)/'data/db/skatercollections.bin'
    if loose.exists():
        return loose.read_bytes()
    try:
        from tools.owned_game.big import BigArchive
        archive = BigArchive(Path(game_root)/'data/big/db.big')
        for entry in archive.entries:
            if entry.path.lower().replace(chr(92), '/') == 'data/db/skatercollections.bin':
                return archive.read(entry)
    except (OSError, ValueError):
        return None
    return None
