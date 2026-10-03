"""Per-bail summary of the recomp's ragdoll body impacts (BAILSTEP / BAILREG, category audiox) and the body posts.

For each bail (BAILSTEP lines less than 1 s apart): the update interval, then per body region (0..7): contact
steps, the parts seen, the impact the game wrote (max, p50, p90, share over the tier-1 / tier-2 band floors),
|dv.n| percentiles (the part's velocity change along the contact normal per update), and for the largest 10 %
of |dv.n| the median signed normal velocity before / after the update plus the share that stop or bounce
(sign change, or |v_new.n| < 0.25 |v_old.n|): a contact stop shows into-surface v_old.n going to ~0 or reversing;
a drive changes v.n without that pattern. Then the body / cloth / concrete Splice posts (SPLC ids, category
audio) in the bail window -0.5 s .. +0.5 s, by region and tier, and an all-bails |dv.n| distribution.

Velocities in the skeleton state are finite differences x 60, so dv is per update; compare against
our per-step dv with the update interval in mind (the `step ms` line).

usage: py -3.13 bail_impacts.py <trace.tsv> [--gap MS]
Refuses a trace with malformed BAIL* lines (bad data is unusable; record again).
"""
import argparse
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from trace import Trace  # noqa: E402

# Body poster ids by region and tier (doc 11 "Bail density: ours rendered, the impact scale checked").
POST_IDS = {
    952: ('head', 0), 953: ('head', 1), 1032: ('head', 2),
    949: ('torso', 0), 950: ('torso', 1), 951: ('torso', 2),
    947: ('legs', 0), 948: ('legs', 1), 1031: ('legs', 2),
    956: ('arms', 0), 957: ('arms', 1), 1030: ('arms', 2),
    954: ('cloth skin', 0), 955: ('cloth denim', 0),
    958: ('concrete', 0), 1047: ('concrete', 1), 991: ('concrete', 2),
}
BANDS = (0.1, 0.25)  # the body records' lowest tier-1 / tier-2 floors (torso, head) for a rough share


def q(values, p):
    if not values:
        return float('nan')
    s = sorted(values)
    return s[min(len(s) - 1, int(p * (len(s) - 1) + 0.5))]


def fnum(text):
    try:
        return float(text)
    except ValueError:
        return float('nan')


def group_bails(steps, gap):
    bails = []
    for ms, f in steps:
        if not bails or ms - bails[-1][-1][0] > gap:
            bails.append([])
        bails[-1].append((ms, f))
    return bails


def summarise(path, gap, out=print):
    t = Trace(path)
    bad = {k: n for k, n in t.malformed().items() if k.startswith('BAIL') and n}
    if bad:
        out(f'REFUSED: malformed BAIL* lines {bad}; the session is unusable, record it again')
        return 2
    steps = t.lines.get('BAILSTEP', [])
    regs = t.lines.get('BAILREG', [])
    out(f'{path}: BAILLOCAL {len(t.lines.get("BAILLOCAL", []))}, BAILSTEP {len(steps)}, BAILREG {len(regs)}')
    for ms, f in t.lines.get('BAILLOCAL', []):
        out(f'  local {ms:.0f} ms: character {f[0]} object {f[1]} X {f[2]} how {f[3]} entry {f[4]}')
    if not steps:
        out('no BAILSTEP lines (no bail of the local player, or the local block was not found)')
        return 1
    splc = [(ms, int(f[1])) for ms, f in t.lines.get('SPLC', []) if len(f) >= 2 and f[1].lstrip('-').isdigit()]
    all_dvn = defaultdict(list)
    for n, bail in enumerate(group_bails(steps, gap), 1):
        a, b = bail[0][0], bail[-1][0]
        step_ms = [fnum(f[1]) for _, f in bail if fnum(f[1]) > 0]
        dts = Counter(f[2] for _, f in bail)
        hows = Counter(f[9] for _, f in bail)
        out(f'\nbail {n}: {a / 1000:.2f}..{b / 1000:.2f} s, {len(bail)} updates; step ms p10/p50/p90 '
            f'{q(step_ms, .1):.2f}/{q(step_ms, .5):.2f}/{q(step_ms, .9):.2f}; dt field {dict(dts)}; how {dict(hows)}; '
            f'bail/end bits {dict(Counter((f[3], f[4]) for _, f in bail))}')
        hitches = [s for s in step_ms if s > 40]
        if hitches:
            out(f'  HITCH: {len(hitches)} updates > 40 ms (max {max(hitches):.0f} ms) inside this bail; mark or exclude '
                f'its dv data')
        if all((f[3], f[4]) == ('0', '0') for _, f in bail):
            out('  NOTE: bail/end bits 0 throughout: opened by another GREC owner\'s bail (an NPC skater), not the local '
                'rider; exclude')
        per = defaultdict(list)
        for ms, f in regs:
            if a - 1 <= ms <= b + 1:  # BAILREG lines precede their BAILSTEP in the same ms
                per[int(f[1])].append(f)
        out('  reg steps parts            imp max   p50   p90 >.10 >.25 | |dv.n| p50   p90   max | top10% v_old.n v_new.n stop')
        for i in sorted(per):
            rows = per[i]
            imp = [fnum(f[3]) for f in rows]
            dvn = [fnum(f[4]) for f in rows]
            all_dvn[i] += dvn
            parts = ','.join(p for p, _ in Counter(f[2] for f in rows).most_common(3))
            cut = q(dvn, .9)
            top = [f for f in rows if fnum(f[4]) >= cut]
            vo = [fnum(f[5]) for f in top]
            vn = [fnum(f[6]) for f in top]
            stop = sum(1 for o, w in zip(vo, vn) if (o * w < 0) or abs(w) < 0.25 * abs(o)) / max(len(top), 1)
            share = [sum(1 for x in imp if x > band) / len(imp) for band in BANDS]
            out(f'  {i:>3} {len(rows):>5} {parts:<16} {max(imp):7.3f} {q(imp, .5):5.3f} {q(imp, .9):5.3f} '
                f'{share[0]:4.0%} {share[1]:4.0%} | {q(dvn, .5):10.3f} {q(dvn, .9):5.3f} {max(dvn):5.3f} | '
                f'{q(vo, .5):14.3f} {q(vn, .5):7.3f} {stop:4.0%}')
        posts = Counter(POST_IDS[i] for ms, i in splc if a - 500 <= ms <= b + 500 and i in POST_IDS)
        by_region = defaultdict(lambda: [0, 0, 0])
        for (region, tier), c in posts.items():
            by_region[region][tier] += c
        out('  posts (tier 0/1/2): ' + ('; '.join(f'{r} {v[0]}/{v[1]}/{v[2]}' for r, v in sorted(by_region.items()))
                                     or 'none (category audio off, or no body post)'))
    out('\nall bails, |dv.n| per region: p50 / p90 / p99 / max (n)')
    for i in sorted(all_dvn):
        v = all_dvn[i]
        out(f'  {i}: {q(v, .5):.3f} / {q(v, .9):.3f} / {q(v, .99):.3f} / {max(v):.3f} ({len(v)})')
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('trace')
    ap.add_argument('--gap', type=float, default=1000.0, help='ms between BAILSTEP lines that starts a new bail')
    args = ap.parse_args()
    return summarise(args.trace, args.gap)


if __name__ == '__main__':
    sys.exit(main())
