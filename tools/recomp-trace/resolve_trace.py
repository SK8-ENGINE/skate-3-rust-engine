"""Resolve sample payloads in a recomp audio trace to (archive, bank member, stream index), by finding
the payload bytes in your own extracted disc's audio archives. Used by retail_voices.py; also runs alone
over the trace's XMA lines.

usage: py -3.13 tools/recomp-trace/resolve_trace.py <trace.tsv> <out.json> [--disc DIR]
  --disc: extracted disc root, the folder holding data/ (default $SKATE3_DISC or .local/skate3-disc).
Writes {"events": [[ms, context, [archive, member, stream] | null], ...]} for every XMA line.
"""
import bisect
import json
import os
import sys
from multiprocessing import Pool
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.owned_game.big import BigArchive  # noqa: E402
from tools.asset_pipeline.audio_formats import scan_snr, splc_streams, grain  # noqa: E402

ROOT = Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')) / 'data/audio'
FILES = ['audiofiles.big', 'ambience.big', 'grains.big', 'wheels.big', 'post.big']
_data = {}


def set_disc(disc: Path):
    """Point the resolver at another extracted disc root."""
    global ROOT
    ROOT = Path(disc) / 'data/audio'


def _init(root=None):
    """Pool initialiser: load the archives into this process (root = the data/audio folder)."""
    base = Path(root) if root else ROOT
    for n in FILES:
        _data[n] = (base / n).read_bytes()


def _find(payload):
    key = bytes.fromhex(payload)
    for n in FILES:
        i = _data[n].find(key)
        if i >= 0:
            return payload, n, i
    return payload, None, -1


def stream_index():
    """Per archive: sorted (entry offset, stored size, member name, stream byte ranges)."""
    out = {}
    for n in FILES:
        a = BigArchive(ROOT / n)
        entries = []
        for e in a.entries:
            d = a.read(e) if e.compression == 0 else None
            name = Path(e.path).name
            ranges = []
            if d is not None:
                try:
                    if name.endswith('.bnk'):
                        ss = splc_streams(d)
                    elif name.endswith('.grain'):
                        ss = [grain(d).stream]
                    elif name.endswith('.sns'):
                        ss = []  # ambience bodies: one stream per file
                    else:
                        ss = scan_snr(d)
                    ranges = [(s.offset, s.end) for s in ss]
                except Exception:
                    ranges = []
            entries.append((e.offset, e.stored_size, name, ranges))
        entries.sort()
        out[n] = entries
    return out


def locate(index, n, pos):
    entries = index[n]
    i = bisect.bisect_right([e[0] for e in entries], pos) - 1
    if i < 0:
        return None
    off, size, name, ranges = entries[i]
    if pos >= off + size:
        return None
    rel = pos - off
    for k, (a, b) in enumerate(ranges):
        if a <= rel < b:
            return name, k
    return name, None


def main():
    args = sys.argv[1:]
    if len(args) < 2 or args[0] in ('-h', '--help'):
        sys.exit(__doc__)
    if '--disc' in args:
        i = args.index('--disc')
        set_disc(Path(args[i + 1]))
        del args[i:i + 2]
    trace, out = Path(args[0]), Path(args[1])
    lines = [l.rstrip('\n').split('\t') for l in trace.open(encoding='utf-8', errors='replace')]
    xma = [l for l in lines if l[0] == 'XMA' and len(l) > 6]
    uniq = sorted({l[6] for l in xma})
    with Pool(processes=8, initializer=_init, initargs=(str(ROOT),)) as pool:
        hits = pool.map(_find, uniq, chunksize=64)
    index = stream_index()
    resolved = {}
    for payload, n, pos in hits:
        resolved[payload] = (n,) + locate(index, n, pos) if n and locate(index, n, pos) else None
    events = [(float(l[1]), int(l[2]), resolved.get(l[6])) for l in xma]
    out.write_text(json.dumps({'events': events}))
    print('xma lines', len(events), 'resolved', sum(1 for e in events if e[2]))


if __name__ == '__main__':
    main()
