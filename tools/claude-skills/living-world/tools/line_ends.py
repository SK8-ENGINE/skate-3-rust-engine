"""How NPC skater lines end: per district, unique paths, how many have no branch group at or after
their last few nodes, where the last branch group sits, line lengths (m, s) and the distance
from a line's last node to the nearest node of any other line.

usage: py -3 line_ends.py <assets>/private/living_world/skater_paths [pipeline dir]
The pipeline dir (default: the main checkout's tools/asset_pipeline) provides the pack reader.
"""
import struct
import sys
from pathlib import Path

folder = Path(sys.argv[1])
pipe = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(__file__).resolve().parents[4] / 'tools' / 'asset_pipeline'
sys.path.insert(0, str(pipe))
import living_world_skaters as lws  # noqa: E402

NODE = lws.NODE_STRIDE


def pct(v, p):
    v = sorted(v)
    return v[min(len(v) - 1, int(p / 100 * len(v)))] if v else 0


for pack in sorted(folder.glob('*.bin')):
    seen = {}
    for _aid, _name, blob in lws.read_pack(pack.read_bytes()):
        count, array = struct.unpack_from('>II', blob, 0)
        for i in range(count):
            h = array + i * lws.PATH_STRIDE
            pid = blob[h + 0x20:h + 0x30].hex()
            if pid in seen:
                continue
            nodes_at, nodes, _ext, groups_at, groups, _flags = struct.unpack_from('>6I', blob, h + 0x30)
            first = h + nodes_at
            pos = [struct.unpack_from('>3f', blob, first + k * NODE) for k in range(nodes)]
            frames = [blob[first + k * NODE + 0x24] for k in range(nodes)]
            gnodes = [struct.unpack_from('>III', blob, h + groups_at + g * lws.BRANCH_GROUP_STRIDE)[2] for g in range(groups)]
            seen[pid] = (pos, frames, gnodes)
    ends_no_branch = 0
    tail_s = []
    lens_s, lens_m = [], []
    allpts = [(pid, p) for pid, (pos, _, _) in seen.items() for p in pos[::4]]
    near_other = []
    near_start, starts_in_4m = [], []
    for pid, (pos, frames, gnodes) in seen.items():
        n = len(pos)
        secs = sum(frames[:-1]) / 60.0
        lens_s.append(secs)
        lens_m.append(sum(sum((a - b) ** 2 for a, b in zip(pos[k], pos[k - 1])) ** .5 for k in range(1, n)))
        last_g = max(gnodes) if gnodes else -1
        if last_g < n - 1:
            ends_no_branch += 1
        tail_s.append(sum(frames[max(last_g, 0):-1]) / 60.0)
        e = pos[-1]
        best = min((sum((a - b) ** 2 for a, b in zip(e, p)) for q, p in allpts if q != pid), default=0) ** .5
        near_other.append(best)
        st = sorted((sum((a - b) ** 2 for a, b in zip(e, v[0][0])) ** .5, q) for q, v in seen.items() if q != pid and v[0])
        near_start.append(st[0][0] if st else 1e9)
        starts_in_4m.append(sum(1 for d, _ in st if d <= 4.0))
    k = len(seen)
    print(f'{pack.stem}: {k} unique lines; last node without a branch group: {ends_no_branch}; '
          f'with no branch group at all: {sum(1 for v in seen.values() if not v[2])}')
    print(f'  length s p10/50/90 {pct(lens_s, 10):.1f}/{pct(lens_s, 50):.1f}/{pct(lens_s, 90):.1f}; '
          f'm {pct(lens_m, 10):.0f}/{pct(lens_m, 50):.0f}/{pct(lens_m, 90):.0f}')
    print(f'  ride after the last branch group to the end, s p10/50/90 '
          f'{pct(tail_s, 10):.1f}/{pct(tail_s, 50):.1f}/{pct(tail_s, 90):.1f}')
    print(f'  end node to the nearest node (every 4th) of another line, m p10/50/90 '
          f'{pct(near_other, 10):.1f}/{pct(near_other, 50):.1f}/{pct(near_other, 90):.1f}')
    print(f'  end node to the nearest START node of another line (sub_82458968 mode 1), m p10/50/90/max '
          f'{pct(near_start, 10):.1f}/{pct(near_start, 50):.1f}/{pct(near_start, 90):.1f}/{max(near_start):.1f}; '
          f'lines with >=1 start within 4 m: {sum(1 for c in starts_in_4m if c)}; candidates within 4 m p50/p90/max '
          f'{pct(starts_in_4m, 50)}/{pct(starts_in_4m, 90)}/{max(starts_in_4m)}')
