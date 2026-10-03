"""Join a recomp session's per-voice lines: PLAY (sample start, resolved to bank/stream) with the MOD
(PITCH / LPF / HPF / SHELF / PEAK), GAIN and SEND lines of the same player until its next PLAY.
Reference reading of our own traces; nothing is shipped.

usage: py -3.13 tools/recomp-trace/retail_voices.py <session dir> [--bank NAME[,NAME]] [--csv OUT] [--modules]
                                                     [--no-disambiguate] [--disc DIR]
The session dir holds trace.tsv. Sample payloads are resolved against your own extracted disc
(--disc, default $SKATE3_DISC or .local/skate3-disc) with resolve_trace.py.
Writes/reads a cache of resolved sample payloads (<session>/play_resolve.json, all matches in
play_resolve_all.json). Byte-identical samples in several banks (Treatments 16/17 = sense_of_speed 3/4)
are attributed by sample address (otherwise Treatments 16/17 read as sense_of_speed).
Output per bank: voices, pitch ratio (PITCH v3) and requested rate (v1) / scale (v2) quantiles,
LPF / HPF cutoffs, gain (product of GAIN targets) × strongest SEND target; --modules also lists the
non-voice module owners (buses) with their PEAK/SHELF settings.
"""
import bisect
import json
import statistics as st
import sys
from collections import Counter, defaultdict
from multiprocessing import Pool
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import resolve_trace as rt  # noqa: E402


def load(session: Path):
    lines = defaultdict(list)
    for raw in (session / 'trace.tsv').open(encoding='utf-8', errors='replace'):
        f = raw.rstrip('\n').split('\t')
        if len(f) < 3 or f[0] not in ('PLAY', 'MOD', 'GAIN', 'SEND', 'ASTATE', 'SKATEB', 'CAPTURE', 'POST', 'SPLC'):
            continue
        try:
            ms = float(f[1])
        except ValueError:
            continue
        lines[f[0]].append((ms, f[2:]))
    return lines


def resolve(session: Path, plays):
    cache_path = session / 'play_resolve.json'
    cache = json.loads(cache_path.read_text()) if cache_path.exists() else {}
    todo = sorted({f[5] for _, f in plays if len(f) > 5} - set(cache))
    if todo:
        index = rt.stream_index()
        with Pool(8, initializer=rt._init, initargs=(str(rt.ROOT),)) as pool:
            for payload, n, pos in pool.imap_unordered(rt._find, todo, chunksize=64):
                hit = rt.locate(index, n, pos) if n else None
                cache[payload] = [Path(hit[0]).stem, hit[1]] if hit else None
        cache_path.write_text(json.dumps(cache))
    return cache


def _find_all(payload):
    """Every bank/stream whose data contains this payload (byte-identical samples exist across banks)."""
    key = bytes.fromhex(payload)
    hits = []
    for n in rt.FILES:
        data = rt._data[n]
        i = data.find(key)
        while i >= 0:
            hits.append((n, i))
            i = data.find(key, i + 1)
    return payload, hits


def disambiguate(session: Path, plays, cache):
    """Byte-identical samples in two banks (Treatments 16/17 = sense_of_speed 3/4) resolve to the
    first bank in the archive. Re-attribute such plays by the voice's sample address (PLAY field 3):
    each bank's samples sit together in memory, so pick the candidate bank whose unambiguous plays
    lie nearest. Cache: <session>/play_resolve_all.json."""
    all_path = session / 'play_resolve_all.json'
    found = json.loads(all_path.read_text()) if all_path.exists() else {}
    todo = sorted({f[5] for _, f in plays if len(f) > 5 and cache.get(f[5])} - set(found))
    if todo:
        index = rt.stream_index()
        with Pool(8, initializer=rt._init, initargs=(str(rt.ROOT),)) as pool:
            for payload, hits in pool.imap_unordered(_find_all, todo, chunksize=64):
                cands = {tuple(c) for c in (rt.locate(index, n, pos) for n, pos in hits) if c}
                found[payload] = sorted([Path(c[0]).stem, c[1]] for c in cands)
        all_path.write_text(json.dumps(found))
    ambiguous = {p for p, c in found.items() if len({b for b, _ in c}) > 1}
    if not ambiguous:
        return {}
    homes = defaultdict(list)  # bank -> sample addresses of its unambiguous plays
    for _, f in plays:
        if len(f) > 5 and cache.get(f[5]) and f[5] not in ambiguous:
            homes[cache[f[5]][0]].append(int(f[2], 16))
    for v in homes.values():
        v.sort()

    def dist(bank, addr):
        v = homes.get(bank)
        if not v:
            return float('inf')
        i = bisect.bisect_left(v, addr)
        return min(abs(v[j] - addr) for j in (i - 1, i) if 0 <= j < len(v))

    fixed = {}
    for _, f in plays:
        if len(f) > 5 and f[5] in ambiguous:
            addr = int(f[2], 16)
            fixed[(f[0], f[2], f[5])] = min(found[f[5]], key=lambda c: dist(c[0], addr))
    return fixed


def q(v, p):
    v = sorted(v)
    return v[min(len(v) - 1, int(p * (len(v) - 1) + 0.5))] if v else float('nan')


def main():
    if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    if '--disc' in sys.argv:
        rt.set_disc(Path(sys.argv[sys.argv.index('--disc') + 1]))
    session = Path(sys.argv[1])
    banks = None
    if '--bank' in sys.argv:
        banks = set(sys.argv[sys.argv.index('--bank') + 1].split(','))
    lines = load(session)
    plays = lines['PLAY']
    cache = resolve(session, plays)
    fixed = disambiguate(session, plays, cache) if '--no-disambiguate' not in sys.argv else {}
    # per player: sorted PLAY times, to bound each voice's lines
    starts = defaultdict(list)
    for ms, f in plays:
        starts[f[0]].append(ms)
    per_player = defaultdict(lambda: defaultdict(list))  # player -> kind -> [(ms, fields)]
    voice_players = set(starts)
    for ms, f in lines['MOD']:
        owner = f[1].lower().replace('0x', '0x')
        per_player[f[1]]['MOD'].append((ms, f))
    for ms, f in lines['GAIN']:
        per_player[f[0]]['GAIN'].append((ms, f))
    for ms, f in lines['SEND']:
        per_player[f[0]]['SEND'].append((ms, f))
    out = defaultdict(lambda: defaultdict(list))
    rows = []
    for ms, f in plays:
        player = f[0]
        hit = cache.get(f[5]) if len(f) > 5 else None
        hit = fixed.get((f[0], f[2], f[5]), hit) if len(f) > 5 else hit
        bank, stream = (hit if hit else (None, None))
        if banks and bank not in banks:
            continue
        nxt = starts[player]
        i = bisect.bisect_right(nxt, ms)
        end = nxt[i] if i < len(nxt) else ms + 30000
        rec = {'ms': ms, 'bank': bank, 'stream': stream, 'pitch': [], 'rate': [], 'scale': [], 'lpf': [], 'hpf': [], 'gain': {}, 'send': 0.0}
        for kind in ('MOD', 'GAIN', 'SEND'):
            for t, g in per_player[player][kind]:
                if t < ms - 1 or t >= end:
                    continue
                if kind == 'MOD':
                    if g[0] == 'PITCH':
                        rec['rate'].append(float(g[3]))
                        rec['scale'].append(float(g[4]))
                        rec['pitch'].append(float(g[5]))
                    elif g[0] == 'LPF':
                        rec['lpf'].append(float(g[3]))
                    elif g[0] == 'HPF':
                        rec['hpf'].append(float(g[3]))
                elif kind == 'GAIN':
                    rec['gain'][g[1]] = max(rec['gain'].get(g[1], 0.0), float(g[2]))
                else:
                    rec['send'] = max(rec['send'], float(g[2]))
        rows.append(rec)
        o = out[bank]
        o['n'].append(1)
        o['pitch'] += [p for p in rec['pitch'] if p > 0]
        o['rate'] += rec['rate']
        o['scale'] += rec['scale']
        o['lpf'] += rec['lpf']
        o['hpf'] += rec['hpf']
        g = 1.0
        for v in rec['gain'].values():
            g *= v
        o['level'].append(g * rec['send'])
        o['streams'].append(stream)
    print(f'{"bank":28s} voices  pitch p10/p50/p90      scale p10/p50/p90    rate modal   LPF p50   HPF p50   level p50/p90')
    for bank, o in sorted(out.items(), key=lambda kv: -len(kv[1]['n'])):
        rate = Counter(round(r) for r in o['rate']).most_common(2)
        print(f'{str(bank):28s} {len(o["n"]):6d}  {q(o["pitch"], .1):.3f}/{q(o["pitch"], .5):.3f}/{q(o["pitch"], .9):.3f}   '
              f'{q(o["scale"], .1):.3f}/{q(o["scale"], .5):.3f}/{q(o["scale"], .9):.3f}   {rate}   '
              f'{q(o["lpf"], .5):8.0f}  {q(o["hpf"], .5):7.0f}   {q(o["level"], .5):.3f}/{q(o["level"], .9):.3f}  '
              f'streams {Counter(o["streams"]).most_common(6)}')
    if '--csv' in sys.argv:
        import csv
        with open(sys.argv[sys.argv.index('--csv') + 1], 'w', newline='') as fh:
            w = csv.writer(fh)
            w.writerow(['ms', 'bank', 'stream', 'pitch_p50', 'scale_p50', 'rate', 'lpf_p50', 'hpf_p50', 'gain', 'send'])
            for r in rows:
                g = 1.0
                for v in r['gain'].values():
                    g *= v
                w.writerow([r['ms'], r['bank'], r['stream'], q(r['pitch'], .5), q(r['scale'], .5), q(r['rate'], .5), q(r['lpf'], .5), q(r['hpf'], .5), g, r['send']])
    if '--modules' in sys.argv:
        non_voice = Counter()
        for ms, f in lines['MOD']:
            if f[1] not in voice_players:
                non_voice[(f[0], f[1])] += 1
        for (kind, owner), n in non_voice.most_common(40):
            vals = [tuple(g[3:6]) for t, g in per_player[owner]['MOD'] if g[0] == kind]
            print(kind, owner, n, Counter(vals).most_common(3))


if __name__ == '__main__':
    main()
