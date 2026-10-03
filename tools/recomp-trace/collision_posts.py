"""Collision sound manager posts (COLLPOST, category audiox of the research hooks) by poster and owner.

The poster names below map the hook's `caller` field as documented in the hooks' COLLPOST line description.

Splits the posts into bail windows (a GREC-local state's +676 / +677 set, from FIRSTHIT paired with the GRECX line of
the same call, plus 0.5 s) and the rest ("riding / idle"), and per window kind prints, per posting function and GREC
owner (the board object whose audio state the poster's object uses; 0 = not a GREC-local player, e.g. an NPC skater or a
prop), the post count and rate, the material pairs and sounds, local72 values and the top caller chains. Also counts
the body-material sounds 955 / 954 / 956 / 947 (denim / skin / arms / legs, tier 0) in the SPLC lines of the same
windows, so the poster's sound field can be checked against the ids actually resolved.

usage: py -3.13 collision_posts.py <trace.tsv> [--top N]
Refuses a trace with malformed COLLPOST lines (bad data is unusable; record again).
"""
import argparse
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from trace import Trace  # noqa: E402

CALLERS = {
    '82496258': 'poster 82496258', '824BA630': 'poster 824BA630', '824BB0E0': 'grind start',
    '824BC188': 'body poster', '824BCEB0': 'first-hit torso', '824BD000': 'deck impact',
    '824E3BE0': 'physics prop', '00000000': 'other',
}
BODY_IDS = {955: 'denim', 954: 'skin', 956: 'arms', 947: 'legs'}


def bail_windows(t):
    """[(start, end)] ms where any GREC-local state had +676 or +677 set, padded by 500 ms."""
    owner = None
    events = []
    rows = []
    for kind in ('GRECX', 'FIRSTHIT'):
        rows += [(ms, kind, f) for ms, f in t.lines.get(kind, [])]
    # Trace keeps per-kind lists; re-merge by time, GRECX before FIRSTHIT at equal ms (same call order).
    rows.sort(key=lambda r: (r[0], 0 if r[1] == 'GRECX' else 1))
    active = {}
    for ms, kind, f in rows:
        if kind == 'GRECX':
            owner = f[0]
        elif owner is not None:
            on = f[3] not in ('0', '-1') or f[4] not in ('0', '-1')
            if active.get(owner) != on:
                active[owner] = on
                events.append((ms, owner, on))
    wins = []
    open_since = {}
    for ms, o, on in events:
        if on:
            open_since[o] = ms
        elif o in open_since:
            wins.append((open_since.pop(o) - 500, ms + 500))
    end = max((ms for ms, _ in t.lines.get('MARK', [])), default=float('inf'))
    wins += [(a - 500, end) for a in open_since.values()]
    wins.sort()
    merged = []
    for a, b in wins:
        if merged and a <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(merged[-1][1], b))
        else:
            merged.append((a, b))
    return merged


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('trace')
    ap.add_argument('--top', type=int, default=3)
    a = ap.parse_args()
    t = Trace(a.trace)
    bad = t.malformed().get('COLLPOST', 0)
    if bad:
        print(f'REFUSED: {bad} malformed COLLPOST lines; the session is unusable, record it again')
        return 2
    posts = t.lines.get('COLLPOST', [])
    if not posts:
        print('no COLLPOST lines (category audiox off, or an old build)')
        return 1
    wins = bail_windows(t)
    first = min(ms for ms, _ in posts)
    marks = [ms for ms, f in t.lines.get('MARK', []) if f and f[0].startswith('script start')]
    t0 = marks[0] if marks else first
    t1 = max(ms for ms, _ in t.lines.get('MARK', [(posts[-1][0], [])]))
    in_bail = lambda ms: any(x <= ms <= y for x, y in wins)  # noqa: E731
    bail_ms = sum(min(y, t1) - max(x, t0) for x, y in wins if y > t0 and x < t1)
    span = {'bail': max(bail_ms, 1.0), 'other': max(t1 - t0 - bail_ms, 1.0)}
    print(f'{a.trace}: {len(posts)} COLLPOST; window {t0 / 1000:.1f}..{t1 / 1000:.1f} s; bail windows '
          f'{len(wins)} ({bail_ms / 1000:.1f} s)')
    groups = defaultdict(list)
    for ms, f in posts:
        if not t0 <= ms <= t1:
            continue
        groups[('bail' if in_bail(ms) else 'other', f[0], f[3])].append(f)
    for key in sorted(groups):
        kind, fn, owner = key
        g = groups[key]
        rate = len(g) / (span[kind] / 1000)
        print(f'\n[{kind}] {CALLERS.get(fn, fn)} owner {owner}: {len(g)} posts, {rate:.2f}/s')
        pairs = Counter((f[5], f[6], f[7], f[8]) for f in g)
        print('  matA matB tierA tierB: ' + ', '.join(f'{p[0]}/{p[1]} t{p[2]}/{p[3]} x{n}' for p, n in pairs.most_common(6)))
        sounds = Counter(int(f[9], 16) for f in g) + Counter(int(f[10], 16) for f in g)
        print('  sound fields: ' + ', '.join(f'{s:X} x{n}' for s, n in sounds.most_common(8)))
        print('  local72: ' + ', '.join(f'{v} x{n}' for v, n in Counter(f[4] for f in g).most_common()))
        print('  objects: ' + ', '.join(f'{v} x{n}' for v, n in Counter(f[1] for f in g).most_common(3)))
        for c, n in Counter(f[15] for f in g).most_common(a.top):
            print(f'  chain {c} x{n}')
    splc = Counter()
    for ms, f in t.lines.get('SPLC', []):
        if t0 <= ms <= t1 and len(f) >= 2 and f[1].isdigit() and int(f[1]) in BODY_IDS:
            splc[('bail' if in_bail(ms) else 'other', int(f[1]))] += 1
    print('\nSPLC body sounds (all owners): ' + ', '.join(
        f'{k} {i} {BODY_IDS[i]} {n} ({n / (span[k] / 1000):.2f}/s)' for (k, i), n in sorted(splc.items())))
    return 0


if __name__ == '__main__':
    sys.exit(main())
