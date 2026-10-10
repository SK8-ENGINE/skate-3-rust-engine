"""DMO hotpoints (RW4 `PEGASUS::RWOBJECTTYPE_HOTPOINTDATA`, type 0x00EB001E) from the per-model RX2s in parkassets.big.

Retail turns them into ped plugin waypoints when a dynamic object spawns (`82C4E128`, `.local/research/peds/
b82-ped-plugin-search.md`, `b86-dmo-hotpoints.md`): type 0 is the prop's root (no waypoint), type 8 is skipped, type 1 a
trash bin, type 2 a newspaper box, any other type a seat; one waypoint per hotpoint, in the model's own frame. Layout
(the in-place fixup `0x82963FE0` relocates only +4), offsets relative to the section:

- header: +0 u32 count, +4 u32 entries offset (16 on disc), +8 / +12 zero; then count x 112-byte records;
- record: +0..+63 a 4x4 f32 row-vector matrix (row 2 = the facing axis retail copies, row 3 = the position), +64 / +80 a
  tiny box around the position, +96 u32 type, +100 u8 (meaning open).

Every RX2 carries at most one HOTPOINTDATA section; it belongs to the RX2's one template (EB000D +104). No fallback:
anything unexpected raises.
"""
import math
import struct

from .dynamic_props import records, sections

HOTPOINTDATA = 0x00EB001E
RECORD_SIZE = 112

# The hotpoint type -> plugin class map of `82C4E128` (data, not code: the engine reads it from the export).
CLASSES = {'0': 'root', '1': 'waypoint_usetrashbin', '2': 'waypoint_newspaperbox', '8': None, 'default': 'waypoint_sit'}


def section_hotpoints(raw, offset, size):
    """The hotpoints of one HOTPOINTDATA section (model frame)."""
    if offset + 16 > len(raw) or offset + size > len(raw):
        raise ValueError('Truncated HOTPOINTDATA section')
    count, entries, zero_a, zero_b = struct.unpack_from('>4I', raw, offset)
    if entries != 16 or zero_a or zero_b or 16 + count * RECORD_SIZE != size:
        raise ValueError('Unexpected HOTPOINTDATA header')
    result = []
    for index in range(count):
        at = offset + 16 + index * RECORD_SIZE
        m = struct.unpack_from('>16f', raw, at)
        if not all(math.isfinite(v) for v in m) or any(abs(m[i] - w) > 1e-4 for i, w in ((3, 0), (7, 0), (11, 0), (15, 1))):
            raise ValueError('Invalid HOTPOINTDATA matrix')
        result.append(dict(type=struct.unpack_from('>I', raw, at + 96)[0], position=list(m[12:15]),
                           axes=[list(m[0:3]), list(m[4:7]), list(m[8:11])], extra=raw[at + 100]))
    return result


def rx2_hotpoints(raw):
    """{template id: hotpoints} of one RX2 (empty without a HOTPOINTDATA section)."""
    found = [(offset, size) for offset, _, size, _, _, kind in sections(raw) if kind == HOTPOINTDATA]
    if not found:
        return {}
    if len(found) > 1:
        raise ValueError('Two HOTPOINTDATA sections in one RX2')
    templates = [f'{struct.unpack_from(">Q", raw, at + 104)[0]:016X}' for _, at, _ in records(raw, 0xEB000D, 160)]
    if len(templates) != 1:
        raise ValueError(f'HOTPOINTDATA in an RX2 with {len(templates)} templates')
    return {templates[0]: section_hotpoints(raw, *found[0])}


def parkassets_hotpoints(game_root, keep=lambda h: h['type'] != 8):
    """{template id: hotpoints} over parkassets.big (the type-8 hotpoints, which retail skips, are left out)."""
    from tools.owned_game.big import BigArchive
    archive = BigArchive(game_root / 'data/content/parkassets.big')
    result = {}
    for entry in archive.entries:
        if not entry.path.lower().endswith('.rx2'):
            continue
        for key, points in rx2_hotpoints(archive.read(entry)).items():
            points = [h for h in points if keep(h)]
            if not points:
                continue
            if key in result and result[key] != points:
                raise ValueError('Conflicting hotpoints for template ' + key)
            result[key] = points
    return result
