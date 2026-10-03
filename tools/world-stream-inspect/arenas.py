"""Shared helpers: walk the RW4 arenas in a district's simulation streams (cSim_*.xsf) straight from a
world<District>.big (or any .big holding *_Sim.xst + cSim_*.xsf, such as missions.big), nothing extracted.

    import arenas
    for stream_file, asset_id, arena in arenas.district_arenas(disc, 'DIST_SkateSchool'):
        for offset, _, size, _, _, type_id in arenas.entries(arena):
            ...

Uses the vendored stream reader (tools/vendor/university/.../skate3_streams.py) for the asset table and
the section decoder.
"""
import os
import struct
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
sys.path.insert(0, str(REPO / 'tools/vendor/university/tools/vanilla_map_extraction/tools'))
from tools.owned_game.big import BigArchive  # noqa: E402
from skate3_streams import read_atoc, _decode_section  # noqa: E402

ARENA_PROCESSOR = 0xAB329A6A
DISTRICTS = ['DIST_SkateSchool', 'DIST_MegaPark', 'DIST_IndustrialSkatePark', 'DIST_StartPark', 'DIST_BlackBoxPark',
             'DIST_DownTownSkatePark', 'DIST_MaloofMoneyCup', 'DIST_University', 'DIST_Industrial', 'DIST_DownTown']


def default_disc() -> Path:
    """Extracted disc root (the folder holding data/): $SKATE3_DISC, else .local/skate3-disc."""
    return Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc'))


def entries(arena: bytes):
    """The arena's dictionary: one (offset, ?, size, align?, ?, type id) tuple of BE u32 per entry."""
    n = struct.unpack_from('>I', arena, 0x20)[0]
    at = struct.unpack_from('>I', arena, 0x30)[0]
    return [struct.unpack_from('>6I', arena, at + 24 * i) for i in range(n)]


def big_arenas(bigpath, only_dirs=None):
    """Yield (dir name, stream file, asset id, arena bytes) for every arena asset of every cSim_*.xsf in a
    .big, using the *_Sim.xst in the same directory."""
    big = BigArchive(Path(bigpath))
    by = defaultdict(list)
    for e in big.entries:
        by[Path(e.path).parent.as_posix()].append(e)
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir) / '_tmp.xst'
        for dname, ents in sorted(by.items()):
            if only_dirs and Path(dname).name not in only_dirs:
                continue
            xst = [e for e in ents if e.path.endswith('_Sim.xst')]
            if not xst:
                continue
            tmp.write_bytes(big.read(xst[0]))
            recs = {r.asset_id: r for r in read_atoc(tmp)}
            seen = set()
            for e in ents:
                if not (Path(e.path).name.startswith('cSim_') and e.path.endswith('.xsf')):
                    continue
                s = big.read(e)
                cur = struct.unpack_from('>I', s, 16)[0]
                while cur + 208 <= len(s):
                    aid = struct.unpack_from('>Q', s, cur)[0]
                    if aid == 0:
                        break
                    st, hs, stride = struct.unpack_from('>III', s, cur + 8)
                    r = recs.get(aid)
                    if r and r.processor_id == ARENA_PROCESSOR and aid not in seen:
                        seen.add(aid)
                        if st == r.total_size and hs >= 128:
                            data = s[cur + hs:cur + hs + st]
                        else:
                            v = struct.unpack_from('>11I', s, cur + 128)
                            c, q = _decode_section(s, cur + 208, v[0], v[1], v[2])
                            g, q = _decode_section(s, q, v[8], v[9], v[10])
                            data = c + g
                        yield Path(dname).name, Path(e.path).name, aid, data
                    if stride == 0:
                        break
                    cur += stride


def district_arenas(disc: Path, district: str):
    """Yield (stream file, asset id, arena bytes) for one world district."""
    for _, fname, aid, data in big_arenas(Path(disc) / 'data/content' / f'world{district}.big'):
        yield fname, aid, data
