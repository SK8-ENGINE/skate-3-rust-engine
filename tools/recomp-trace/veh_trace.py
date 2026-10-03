"""Traffic summary from a trace with hooks_traffic.cpp lines (VEHSTATE, TRAF*, PEDHONKED).

usage: py -3.13 tools/recomp-trace/veh_trace.py <trace.tsv> [--vehicle ADDR]
Per vehicle: samples, time span, horizontal speed from positions (median / p90 / max), target speed (+3408)
range, manoeuvre (+4396) and target-lane (+4380) changes, stops (speed < 0.3 m/s for ≥ 1 s). Then the
first-pass TRAF* lines with times (horn, skid, engine, light, phase lengths) and screenshot names.
--vehicle prints one vehicle's samples (t, target, manoeuvre, lane, speed, position).
"""
import math
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from trace import Trace  # noqa: E402


def samples(t: Trace):
    out = defaultdict(list)
    for ms, f in t.lines.get('VEHSTATE', []):
        if len(f) < 9:
            continue
        try:
            pos = [float(v) for v in f[7].split()]
            out[f[0]].append((ms / 1000, float(f[1]), int(f[2]), int(f[3]), pos))
        except (ValueError, IndexError):
            continue
    return out


def pct(values, q):
    v = sorted(values)
    return v[min(len(v) - 1, int(len(v) * q))] if v else float('nan')


def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    t = Trace(sys.argv[1])
    vehicles = samples(t)
    if '--vehicle' in sys.argv:
        key = sys.argv[sys.argv.index('--vehicle') + 1].upper()
        prev = None
        for s in vehicles.get(key, []):
            speed = math.dist(s[4][::2], prev[4][::2]) / (s[0] - prev[0]) if prev and s[0] > prev[0] else 0
            print(f'{s[0]:8.2f} target {s[1]:6.2f} man {s[2]} lane {s[3]} speed {speed:6.2f} pos {s[4]}')
            prev = s
        return
    print(f'{len(vehicles)} vehicles')
    all_speeds = []
    for key, ss in sorted(vehicles.items(), key=lambda kv: -len(kv[1])):
        speeds, stops, still_since = [], [], None
        for a, b in zip(ss, ss[1:]):
            dt = b[0] - a[0]
            if not 0.1 < dt < 1.0:
                continue
            v = math.dist(a[4][::2], b[4][::2]) / dt
            if v > 60:
                continue  # spawn / teleport jump
            speeds.append(v)
            if v < 0.3:
                still_since = still_since if still_since is not None else a[0]
            else:
                if still_since is not None and a[0] - still_since >= 1.0:
                    stops.append((still_since, a[0]))
                still_since = None
        all_speeds += speeds
        mans = [(s[0], s[2]) for i, s in enumerate(ss) if i and s[2] != ss[i - 1][2]]
        lanes = [(s[0], s[3]) for i, s in enumerate(ss) if i and s[3] != ss[i - 1][3]]
        targets = [s[1] for s in ss]
        print(f'== {key}: {len(ss)} samples {ss[0][0]:.1f}-{ss[-1][0]:.1f} s; speed median {pct(speeds, .5):.2f} '
              f'p90 {pct(speeds, .9):.2f} max {max(speeds, default=0):.2f} m/s; target {min(targets):.2f}..{max(targets):.2f}')
        for a, b in stops:
            print(f'     stop {a:.1f}-{b:.1f} s ({b - a:.1f} s)')
        for when, m in mans:
            print(f'     manoeuvre -> {m} at {when:.2f} s')
        for when, l in lanes:
            print(f'     target lane -> {l} at {when:.2f} s')
    moving = [v for v in all_speeds if v > 0.5]
    print(f'all vehicles moving: n {len(moving)} median {pct(moving, .5):.2f} p90 {pct(moving, .9):.2f} m/s')
    for kind in ('TRAFHORN', 'TRAFSKID', 'TRAFLIGHT', 'TRAFPHASE', 'PEDHONKED'):
        lines = t.lines.get(kind, [])
        print(f'-- {kind}: {len(lines)}')
        for ms, f in lines[:30]:
            fl = f[2].split() if len(f) > 2 else []
            shot = t.shot_for(ms)
            print(f'   {ms / 1000:8.2f} f1 {fl[0] if fl else "?"} callers {f[-1] if f else ""}' + (f'  [{shot.name}]' if shot else ''))


if __name__ == '__main__':
    main()
