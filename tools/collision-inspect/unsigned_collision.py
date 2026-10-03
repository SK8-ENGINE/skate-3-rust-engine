"""Make the vendored collision decoder read 16-bit vertex deltas UNSIGNED, as the game does.

The vendored `retail_collision_mesh._decode_vertices` unpacks compression-1 vertex deltas with `>3h`
(signed). The game's own decoder (crates/skate-data/src/retail_collision.rs, `be16` → u16) treats them as
unsigned, as the game does. With signed deltas, vertices of clusters spanning more than 32.8 m wrap by
65.536 m (granularity 0.001); see docs/hails-additions/05-collision-volumes.md.

Analysis tools only: `import unsigned_collision` before using `decode_rx2_clustered_meshes`. The converter
(map_writer.spawn_point, prepare_hawaiian_dream bounds) still uses the vendored behaviour.
"""
import struct

import retail_collision_mesh as _rcm


def _decode_vertices_unsigned(cluster, vertex_count, compression, granularity, vertex_data_end):
    if compression != 1:
        return _original(cluster, vertex_count, compression, granularity, vertex_data_end)
    _rcm._require(vertex_data_end >= 28, "compressed cluster omits its base")
    base = struct.unpack_from(">3i", cluster, _rcm.CLUSTER_HEADER_SIZE)
    vertices = []
    for index in range(vertex_count):
        offset = _rcm.CLUSTER_HEADER_SIZE + 12 + index * 6
        _rcm._require(offset + 6 <= vertex_data_end, "16-bit compressed vertex extends into the unit stream")
        delta = struct.unpack_from(">3H", cluster, offset)
        vertices.append(tuple(_rcm._f32((base[axis] + delta[axis]) * granularity) for axis in range(3)))
    return vertices


_original = _rcm._decode_vertices
_rcm._decode_vertices = _decode_vertices_unsigned
