"""Dump every named volume (arena type 0x00EB0019, 'volume set') in the world districts: trigger boxes,
teleport and zone volumes, with their bounds, base corners and link ids.

usage: py -3.13 tools/world-stream-inspect/volumes_dump.py [DIST_x ...] [--disc DIR] [--json OUT] > volumes.txt
Default: all ten districts; JSON to .local/world-stream-inspect/volumes.json.
Volume-set layout (big-endian):
  header: +0 magic, +4 count, +8 count, +12 items offset, +16 strings offset
  item (240 bytes): +0 mat4, +64 aabb min (vec4), +80 aabb max, +96 up axis (vec4), +112 4 base corners (vec4 x4),
  +176 u64 link GUID (an import of type 0x00EB006B), +184 u64 instance id, +192 16 bytes 0xFF, +220 import handle,
  +224 name offset (from record start).
Each arena line also lists its clustered collision meshes (triangles, surfaceless or not) and other entry types.
"""
import argparse
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import arenas  # noqa: E402
from retail_collision_mesh import decode_clustered_mesh  # noqa: E402  (vendored, on the path via arenas)

VOLUME_SET = 0x00EB0019
CLUSTERED_MESH = 0x00080006


def cstr(b, o):
    e = b.index(b'\0', o)
    return b[o:e].decode('latin1')


def items(b):
    magic, n, n2, ioff, soff = struct.unpack_from('>5I', b, 0)
    for i in range(n):
        o = ioff + 240 * i
        mn = struct.unpack_from('>3f', b, o + 64)
        mx = struct.unpack_from('>3f', b, o + 80)
        up = struct.unpack_from('>3f', b, o + 96)
        corners = [struct.unpack_from('>3f', b, o + 112 + 16 * k) for k in range(4)]
        guid, iid = struct.unpack_from('>QQ', b, o + 176)
        imp, noff = struct.unpack_from('>II', b, o + 220)
        mat = struct.unpack_from('>16f', b, o)
        yield dict(name=cstr(b, noff), id='%016x' % iid, link='%016x' % guid, import_handle=imp,
                   min=[round(v, 3) for v in mn], max=[round(v, 3) for v in mx], up=[round(v, 3) for v in up],
                   corners=[[round(v, 3) for v in c] for c in corners],
                   translation=[round(v, 3) for v in mat[12:15]],
                   identity_rot=mat[0] == 1.0 and mat[5] == 1.0 and mat[10] == 1.0)


def main():
    parser = argparse.ArgumentParser(description='Dump every named volume in the world districts.')
    parser.add_argument('districts', nargs='*', help='e.g. DIST_SkateSchool (default: all ten)')
    parser.add_argument('--disc', type=Path, default=arenas.default_disc(), help='extracted disc root (default: %(default)s)')
    parser.add_argument('--json', type=Path, default=arenas.REPO / '.local/world-stream-inspect/volumes.json',
                        help='JSON output (default: %(default)s)')
    args = parser.parse_args()
    out = {}
    for dist in args.districts or arenas.DISTRICTS:
        res = []
        for fname, aid, d in arenas.district_arenas(args.disc, dist):
            ents = arenas.entries(d)
            vs = [e for e in ents if e[5] == VOLUME_SET]
            if not vs:
                continue
            meshes = []
            for e in ents:
                if e[5] == CLUSTERED_MESH:
                    m = decode_clustered_mesh(d[e[0]:e[0] + e[2]])
                    sl = all(not (t.unit_flags & 0x80) for t in m.triangles)
                    meshes.append(dict(tris=len(m.triangles), surfaceless=sl,
                                       min=[round(v, 2) for v in m.bounds_min], max=[round(v, 2) for v in m.bounds_max]))
            other = sorted({'%08x' % e[5] for e in ents} - {'00eb0019', '00080006', '00080001', '00eb000a', '00eb0008', '00eb000b'})
            for e in vs:
                for it in items(d[e[0]:e[0] + e[2]]):
                    it.update(stream=fname, arena='%016x' % aid)
                    res.append(it)
                    short = it['name'].split('_0x')[0]
                    print(dist, fname, it['id'], short, it['min'], it['max'], 'link', it['link'])
            print('   arena %016x meshes %s other types %s' % (aid, [(m['tris'], m['surfaceless']) for m in meshes], other))
        out[dist] = res
    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(out, indent=1))


if __name__ == '__main__':
    main()
