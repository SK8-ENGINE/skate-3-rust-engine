"""Living world road network, version 2: lanes, junctions and turn connectors decoded from the road
network objects (RW type ``0x00EB0013``) the setup reads from the user's disc.

``decode_object(blob)`` decodes one object; ``build(objects)`` merges the per-tile copies of a district
into one graph; ``write_graph`` / ``read_graph`` are the ``roads.bin`` v2 container that
``skate-data::roads`` parses. Layout facts are labelled [data] (checked on all 82 objects of the
shipped districts); field meanings that are not proven keep raw names (``u32_08``, ``f32_50``) so a
key stays stable once the meaning is found.

Object layout (big-endian; offsets from the object start unless "relative") [data]:

``+0x00`` vec4 bbox min, ``+0x10`` vec4 bbox max, ``+0x20`` u32 junction count, u32 lane-run count,
u32 segment count, u32 junction table offset (stale when the count is 0), u32 lane-run table offset,
u32 segment table offset (see ``living_world.parse_roads`` for the 0x40-byte segment records).

Junction (0x260, one per road node that sits in the tile, stored back to back from the junction table offset;
their connectors, arc tables and lane lists follow after all of them):
  ``+0x00`` inner quad: 4 x (vec3 corner, u32 tag), ``+0x40`` outer quad (4 m larger on every side;
  connector ends lie on its edges), ``+0x80`` 8 end records of 0x38 bytes: records 0-3 are the
  approaches of node ends 0-3 (traffic entering the junction), records 4-7 the exits of ends 0-3
  (traffic leaving it), ``+0x240`` connector block header (0x20): f32 ``speed`` (m/s; 13.889 or
  14.167, the road speed), u32 ``flag_04`` (0 / 1), u64 node id, u32 connector offset (relative to the
  junction), u32 connector count, then the connectors (0x70 each), their arc-length tables and the
  connector index lists the end records point at.

End record (0x38): u64 id, u32 ``kind`` (1 = a road, 2 = none, 0 = the node itself in a lane-run end),
u32 ``side`` (1 = approach, 0 = exit, 0xFFFFFFFF = none; the node end index in a lane-run end at the
node), u32 lane count, u32 count[4] (connectors per lane), u32 offset[4] (relative to the record: a
list of u32 connector indices per lane; unused slots hold stale memory), pad.

Connector (0x70; a cubic Hermite curve): vec4 start, vec4 end, vec4 start tangent, vec4 end tangent,
f32 length, u32 arc sample count (16), u32 arc table offset (relative to the connector), pad, f32
``f32_50``, u32 from end, u32 from lane, u32 to end, u32 to lane, pad. The arc table holds the
cumulative length at 16 evenly spaced curve parameters (0 .. length).

Lane run (0xA0; one per segment per tile): u64 id, end record ``start`` (+0x08), end record ``end``
(+0x40; at a junction its lists give the connectors each lane may take), u32 piece count, u32 piece
offset (relative to the run), f32 speed limit, u32 lane count, f32 road width, f32 run length
(the pieces in this tile), f32 distance at the run end, u32 index of the first piece in the whole
segment, u64 segment id.

Piece (0xE0; about 4 m of road): the centre-line curve (vec4 start, vec4 end, vec4 start tangent,
vec4 end tangent, f32 length, u32 arc sample count, u32 arc table offset relative to the piece, pad),
the road edges (vec4 left start, right start, left end, right end; vec4 tangents of the same four),
f32 cumulative length at the piece end (within the run), pad. Pieces run in the direction of travel,
from the segment's ``node_b`` to its ``node_a`` (the piece next to ``node_a`` has the highest index)
[data: geometry of every lane run], so ``node_a`` is the segment's destination.
"""
from __future__ import annotations

import struct

GRAPH_MAGIC = b'LWROADS2'
GRAPH_VERSION = 2
JUNCTION_HEADER = 0x240
JUNCTION_SIZE = 0x260  # junction records are stored back to back; connectors and lists follow them all
END_RECORDS = 8
END_SIZE, CONNECTOR_SIZE, RUN_SIZE, PIECE_SIZE = 0x38, 0x70, 0xA0, 0xE0


def _round(value: float) -> float:
    from .living_world import _round as shortest
    return shortest(value)


def _vec3(blob: bytes, at: int) -> list[float]:
    return [_round(v) for v in struct.unpack_from('>3f', blob, at)]


def _arc(blob: bytes, at: int, count: int) -> list[float]:
    if count > 64 or at + 4 * count > len(blob):
        raise ValueError(f'arc table at {at:#x} ({count}) runs past the object end')
    return [_round(v) for v in struct.unpack_from(f'>{count}f', blob, at)]


def _curve(blob: bytes, at: int) -> dict:
    length, samples, table = struct.unpack_from('>fII', blob, at + 0x40)
    return {'start': _vec3(blob, at), 'end': _vec3(blob, at + 0x10), 'tangent_start': _vec3(blob, at + 0x20),
            'tangent_end': _vec3(blob, at + 0x30), 'length': _round(length), 'arc': _arc(blob, at + table, samples)}


def _end_record(blob: bytes, at: int) -> dict:
    rid, kind, side, lanes = struct.unpack_from('>QIII', blob, at)
    counts = struct.unpack_from('>4I', blob, at + 0x14)
    offsets = struct.unpack_from('>4I', blob, at + 0x24)
    record = {'id': f'{rid:016X}', 'kind': kind, 'side': side, 'lanes': lanes, 'connectors': []}
    if kind == 2:  # no road on this end
        return record
    if lanes > 4:
        raise ValueError(f'end record at {at:#x}: {lanes} lanes')
    for lane in range(lanes):
        count = counts[lane]
        if count == 0:
            record['connectors'].append([])
            continue
        start = at + offsets[lane]
        if count > 32 or start + 4 * count > len(blob):
            raise ValueError(f'end record at {at:#x}: lane {lane} list runs past the object end')
        record['connectors'].append(list(struct.unpack_from(f'>{count}I', blob, start)))
    return record


def _junction(blob: bytes, at: int) -> tuple[dict, int]:
    """One junction and the offset just past its connectors."""
    quads = []
    for q in range(2):
        corners, tags = [], []
        for k in range(4):
            corners.append(_vec3(blob, at + 0x40 * q + 16 * k))
            tags.append(struct.unpack_from('>I', blob, at + 0x40 * q + 16 * k + 12)[0])
        quads.append((corners, tags))
    ends = [_end_record(blob, at + 0x80 + END_SIZE * i) for i in range(END_RECORDS)]
    head = at + JUNCTION_HEADER
    speed, flag, node, offset, count = struct.unpack_from('>fIQII', blob, head)
    if count > 64 or at + offset + CONNECTOR_SIZE * count > len(blob):
        raise ValueError(f'junction at {at:#x}: {count} connectors run past the object end')
    connectors = []
    for i in range(count):
        c = at + offset + CONNECTOR_SIZE * i
        curve = _curve(blob, c)
        f50, from_end, from_lane, to_end, to_lane = struct.unpack_from('>fIIII', blob, c + 0x50)
        connectors.append({'index': i, **curve, 'f32_50': _round(f50), 'from_end': from_end, 'from_lane': from_lane,
                           'to_end': to_end, 'to_lane': to_lane})
    junction = {'node': f'{node:016X}', 'inner': quads[0][0], 'outer': quads[1][0],
                'quad_tags': [quads[0][1][0], quads[1][1][0]], 'speed': _round(speed), 'flag_04': flag,
                'approaches': ends[:4], 'exits': ends[4:], 'connectors': connectors}
    return junction, at + offset + CONNECTOR_SIZE * count


def _run(blob: bytes, at: int) -> dict:
    rid = struct.unpack_from('>Q', blob, at)[0]
    pieces_count, pieces_at, speed, lanes, width, run_length, end_distance, first, segment = struct.unpack_from(
        '>IIfIfffIQ', blob, at + 0x78)
    if pieces_count > 256 or at + pieces_at + PIECE_SIZE * pieces_count > len(blob):
        raise ValueError(f'lane run at {at:#x}: {pieces_count} pieces run past the object end')
    pieces = []
    for i in range(pieces_count):
        p = at + pieces_at + PIECE_SIZE * i
        curve = _curve(blob, p)
        edges = [_vec3(blob, p + 0x50 + 16 * k) for k in range(4)]
        edge_tangents = [_vec3(blob, p + 0x90 + 16 * k) for k in range(4)]
        pieces.append({'index': first + i, **curve, 'left_start': edges[0], 'right_start': edges[1],
                       'left_end': edges[2], 'right_end': edges[3], 'edge_tangents': edge_tangents,
                       'distance': _round(struct.unpack_from('>f', blob, p + 0xD0)[0])})
    return {'id': f'{rid:016X}', 'segment': f'{segment:016X}', 'start': _end_record(blob, at + 0x08),
            'end': _end_record(blob, at + 0x40), 'speed_limit': _round(speed), 'lanes': lanes, 'width': _round(width),
            'run_length': _round(run_length), 'end_distance': _round(end_distance), 'first_piece': first,
            'pieces': pieces}


def decode_object(blob: bytes) -> dict:
    """Junctions, lane runs and segments of one road network object."""
    from .living_world import parse_roads
    head = parse_roads(blob)
    junction_count, run_count, _, junctions_at, runs_at, _ = struct.unpack_from('>6I', blob, 0x20)
    if junction_count and junctions_at + JUNCTION_SIZE * junction_count > len(blob):
        raise ValueError('junction table runs past the object end')
    junctions = [_junction(blob, junctions_at + JUNCTION_SIZE * i)[0] for i in range(junction_count)]
    if runs_at + RUN_SIZE * run_count > len(blob):
        raise ValueError('lane-run table runs past the object end')
    runs = [_run(blob, runs_at + RUN_SIZE * i) for i in range(run_count)]
    return {'bbox': head['bbox'], 'segments': head['segments'], 'junctions': junctions, 'runs': runs}


# ---------------------------------------------------------------- district graph

def build(objects: list[bytes]) -> dict:
    """Merge the decoded objects of one district (every tile holds a copy of the segments, junctions
    and lane runs that touch it) into one graph: ``segments`` (by id, with ``to_node`` / ``from_node``,
    their pieces in travel order and the distance along the segment at each piece end), ``junctions``
    (by node id) and ``problems`` (copies that disagree, pieces missing)."""
    segments, junctions, pieces, problems = {}, {}, {}, []
    for blob in objects:
        decoded = decode_object(blob)
        for s in decoded['segments']:
            known = segments.setdefault(s['id'], dict(s))
            if known != s:
                problems.append(f"copies of segment {s['id']} differ")
        for j in decoded['junctions']:
            known = junctions.setdefault(j['node'], j)
            if known != j:
                problems.append(f"copies of junction {j['node']} differ")
        for run in decoded['runs']:
            start = run['end_distance'] - run['run_length']  # distance along the segment at the run start
            for piece in run['pieces']:
                entry = {**piece, 'distance': _round(start + piece['distance'])}
                known = pieces.setdefault(run['segment'], {}).setdefault(piece['index'], entry)
                if any(abs(a - b) > 2e-3 for a, b in zip(known['start'] + known['end'], entry['start'] + entry['end'])):
                    problems.append(f"copies of piece {piece['index']} of segment {run['segment']} differ")
    out_segments = {}
    for sid, s in sorted(segments.items()):
        own = [pieces.get(sid, {})[k] for k in sorted(pieces.get(sid, {}))]
        complete = bool(own) and [p['index'] for p in own] == list(range(len(own))) \
            and abs(own[-1]['distance'] - s['length']) < 0.05
        if not complete:
            problems.append(f'segment {sid}: pieces incomplete ({len(own)})')
        out_segments[sid] = {**s, 'to_node': s['node_a'], 'to_end': s['end_a'], 'from_node': s['node_b'],
                             'from_end': s['end_b'], 'lanes': s['word_52'], 'pieces_complete': complete, 'pieces': own}
    return {'segments': out_segments, 'junctions': dict(sorted(junctions.items())), 'problems': problems}


# ---------------------------------------------------------------- roads.bin v2

# Little-endian records, written in this order after the header (see ``write_graph``).
_HEADER = struct.Struct('<8s7I')                     # magic, version, districts, segments, pieces, junctions, connectors, lane entries
_DISTRICT = struct.Struct('<16s4I')                  # name, first segment, segment count, first junction, junction count
_SEGMENT = struct.Struct('<3Q2I4f6I')                # id, to node, from node, to end, from end, length, width a, width b, speed limit,
                                                     # lanes, word_56, first piece, piece count, flags (bit 0 = pieces complete), district
_PIECE = struct.Struct('<2I2f' + '3f' * 12 + '16f')  # segment, index, distance at the end, length, start, end, tangent start, tangent end,
                                                     # left start, right start, left end, right end, 4 edge tangents, arc table
_JUNCTION = struct.Struct('<QIfIII24f2I')            # node, district, speed, flag_04, inner tag, outer tag, inner + outer quads, first connector, count
_END = struct.Struct('<Q3I4II')                      # id, kind, side, lanes, connectors per lane, first lane entry
_CONNECTOR = struct.Struct('<2I2f4I12f16f')          # junction, index, length, f32_50, from end, from lane, to end, to lane, start, end, tangents, arc
ARC_SAMPLES = 16
SEGMENT_PIECES_COMPLETE = 1


def _arc16(values: list[float]) -> list[float]:
    if len(values) != ARC_SAMPLES:
        raise ValueError(f'arc table has {len(values)} samples, not {ARC_SAMPLES}')
    return values


def write_graph(districts: dict[str, dict]) -> bytes:
    """``roads.bin`` v2: every district graph from ``build`` in one little-endian file (header, then
    districts, segments, pieces, junctions each followed by its 8 end records, connectors, lane
    entries). Ids are the retail 64-bit ids; segments and junctions are sorted by id within a district
    (connectors keep their retail index order), so the file is deterministic."""
    district_rows, segment_rows, piece_rows, junction_rows, connector_rows, lanes = [], [], [], [], [], []
    for d_index, (name, graph) in enumerate(sorted(districts.items())):
        district_rows.append(_DISTRICT.pack(name.encode('ascii')[:16], len(segment_rows), len(graph['segments']),
                                            len(junction_rows), len(graph['junctions'])))
        for sid, s in graph['segments'].items():
            segment_rows.append(_SEGMENT.pack(int(sid, 16), int(s['to_node'], 16), int(s['from_node'], 16), s['to_end'],
                                              s['from_end'], s['length'], s['width_a'], s['width_b'], s['speed_limit'],
                                              s['lanes'], s['word_56'], len(piece_rows), len(s['pieces']),
                                              SEGMENT_PIECES_COMPLETE if s['pieces_complete'] else 0, d_index))
            for p in s['pieces']:
                piece_rows.append(_PIECE.pack(len(segment_rows) - 1, p['index'], p['distance'], p['length'], *p['start'],
                                              *p['end'], *p['tangent_start'], *p['tangent_end'], *p['left_start'],
                                              *p['right_start'], *p['left_end'], *p['right_end'],
                                              *[v for t in p['edge_tangents'] for v in t], *_arc16(p['arc'])))
        for node, j in graph['junctions'].items():
            ends = b''
            for end in j['approaches'] + j['exits']:
                counts = [len(lane) for lane in end['connectors']] + [0] * (4 - len(end['connectors']))
                ends += _END.pack(int(end['id'], 16), end['kind'], end['side'], end['lanes'], *counts, len(lanes))
                for lane in end['connectors']:
                    lanes.extend(lane)
            junction_rows.append(_JUNCTION.pack(int(node, 16), d_index, j['speed'], j['flag_04'], *j['quad_tags'],
                                                *[v for c in j['inner'] + j['outer'] for v in c], len(connector_rows),
                                                len(j['connectors'])) + ends)
            for c in j['connectors']:
                connector_rows.append(_CONNECTOR.pack(len(junction_rows) - 1, c['index'], c['length'], c['f32_50'],
                                                      c['from_end'], c['from_lane'], c['to_end'], c['to_lane'],
                                                      *c['start'], *c['end'], *c['tangent_start'], *c['tangent_end'],
                                                      *_arc16(c['arc'])))
    head = _HEADER.pack(GRAPH_MAGIC, GRAPH_VERSION, len(district_rows), len(segment_rows), len(piece_rows),
                        len(junction_rows), len(connector_rows), len(lanes))
    return b''.join([head, *district_rows, *segment_rows, *piece_rows, *junction_rows, *connector_rows,
                     struct.pack(f'<{len(lanes)}I', *lanes)])


def read_graph(data: bytes) -> dict:
    """Inverse of ``write_graph`` (tests and validation): flat record tuples per section."""
    magic, version, nd, ns, n_pieces, nj, nc, nl = _HEADER.unpack_from(data, 0)
    if magic != GRAPH_MAGIC or version != GRAPH_VERSION:
        raise ValueError('not a roads.bin v2 graph')
    at = _HEADER.size
    expected = at + nd * _DISTRICT.size + ns * _SEGMENT.size + n_pieces * _PIECE.size \
        + nj * (_JUNCTION.size + 8 * _END.size) + nc * _CONNECTOR.size + 4 * nl
    if expected != len(data):
        raise ValueError(f'roads.bin v2 size {len(data)} != {expected}')

    def rows(record, count):
        nonlocal at
        out = [record.unpack_from(data, at + record.size * i) for i in range(count)]
        at += record.size * count
        return out
    districts = rows(_DISTRICT, nd)
    segments = rows(_SEGMENT, ns)
    pieces = rows(_PIECE, n_pieces)
    junctions = []
    for _ in range(nj):
        head = _JUNCTION.unpack_from(data, at)
        at += _JUNCTION.size
        ends = [_END.unpack_from(data, at + _END.size * k) for k in range(8)]
        at += 8 * _END.size
        junctions.append((head, ends))
    connectors = rows(_CONNECTOR, nc)
    lanes = list(struct.unpack_from(f'<{nl}I', data, at))
    return {'districts': [(d[0].rstrip(b'\0').decode('ascii'), *d[1:]) for d in districts], 'segments': segments,
            'pieces': pieces, 'junctions': junctions, 'connectors': connectors, 'lanes': lanes}


def summary(graph: dict) -> dict:
    """``roads.json`` view of a district graph: segments without their pieces (piece count instead) and
    junctions with their connectors (arc tables left to ``roads.bin``)."""
    segs = {sid: {k: v for k, v in s.items() if k != 'pieces'} | {'piece_count': len(s['pieces'])}
            for sid, s in graph['segments'].items()}
    juncs = {node: {**j, 'connectors': [{k: v for k, v in c.items() if k != 'arc'} for c in j['connectors']]}
             for node, j in graph['junctions'].items()}
    return {'segments': segs, 'junctions': juncs}

