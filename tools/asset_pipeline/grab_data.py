"""Authored grab splines (RW4 `PEGASUS::RWOBJECTTYPE_GRABDATA`, type 0x00EB001F) from an RX2.

Retail uses them for skitching (car model parts in livingworld.big, one rear-edge spline per car,
grab record type 1) and for DMO props (parkassets.big). Layout (`.local/research/npc/b35-car-definition-resource.md`;
the in-place fixup is `0x82961590`), offsets relative to the section:

- header: +0 entry count, +4 total control points, +12 count of enabled entries (byte +70 non-zero; only those
  have a direction: 3 parkassets props have disabled entries), +16 entries, +20 points, +24 directions;
- entry (80 bytes): +0 box min (vec4), +16 box max (vec4), +48 points offset, +56 direction offset,
  +60 flags, +68 u8 control-point count, +70 u8 (retail skips an entry with +68 or +70 zero);
- points: vec4 f32, chains of cubic Bezier segments (4 control points each); straight runs repeat
  their control points.

No fallback: anything unexpected raises.
"""
import struct

from .dynamic_props import sections

GRABDATA = 0x00EB001F
ENTRY_SIZE = 80


def grab_splines(raw):
    """Every grab spline of an RX2 as dicts (model frame)."""
    result = []
    for offset, _, size, _, _, kind in sections(raw):
        if kind == GRABDATA:
            result.extend(section_splines(raw, offset, size))
    return result


def section_splines(raw, offset, size):
    """The grab splines of one GRABDATA section (multi-template DMO arenas link one section per template)."""
    result = []
    if offset + 28 > len(raw) or offset + size > len(raw):
        raise ValueError('Truncated GRABDATA section')
    count, total, _, enabled_count, entries, points, directions = struct.unpack_from('>7I', raw, offset)
    if enabled_count > count or entries + count * ENTRY_SIZE > size or points + total * 16 > size or directions + enabled_count * 16 > size:
        raise ValueError('Unexpected GRABDATA header')
    if sum(1 for index in range(count) if raw[offset + entries + index * ENTRY_SIZE + 70]) != enabled_count:
        raise ValueError('GRABDATA enabled count does not match its entries')
    for index in range(count):
        at = offset + entries + index * ENTRY_SIZE
        bmin = struct.unpack_from('>3f', raw, at)
        bmax = struct.unpack_from('>3f', raw, at + 16)
        point_offset = struct.unpack_from('>I', raw, at + 48)[0]
        direction_offset = struct.unpack_from('>I', raw, at + 56)[0]
        flags = struct.unpack_from('>I', raw, at + 60)[0]
        n, enabled = raw[at + 68], raw[at + 70]
        if n == 0 or enabled == 0:
            continue
        if n % 4 != 0 or point_offset + n * 16 > size or direction_offset + 16 > size:
            raise ValueError('Unexpected GRABDATA entry')
        control = [list(struct.unpack_from('>3f', raw, offset + point_offset + k * 16)) for k in range(n)]
        result.append(dict(
            control_points=control,
            direction=list(struct.unpack_from('>3f', raw, offset + direction_offset)),
            bounds=[list(bmin), list(bmax)],
            flags=flags,
        ))
    return result
