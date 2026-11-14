"""Ped facing turn rates from PEDFACE trace lines (hooks_npc.cpp, 2026-10-16).

Usage: python -I ped_face_rate.py <trace.tsv> [more traces...]

PEDFACE <ms> ped | fx fy fz | px py pz | turn | trans | sx sy sz | steer_age_ms
turn / trans: 1 = none running, 0 = a turn clip / transition runs. Steering: the last normalised NavPower steering
direction the facing chooser read (age in ms, -1 = none).

Prints, per state (steering vs turn clip): the facing yaw-rate percentiles; and for steering samples the yaw rate
binned by the facing-to-steering angle (gives the steering law: a constant cap shows a flat rate across bins, a
proportional law a rate rising with the angle). Sanity: facing length and speed.
"""
import math
import sys
from collections import defaultdict


def load(paths):
    peds = defaultdict(list)
    bad = 0
    for path in paths:
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                if not line.startswith("PEDFACE\t"):
                    continue
                p = line.rstrip("\n").split("\t")
                if len(p) != 9:
                    bad += 1
                    continue
                try:
                    ms = float(p[1])
                    fx, fy, fz = map(float, p[3].split())
                    px, py, pz = map(float, p[4].split())
                    turn, trans = int(p[5]), int(p[6])
                    sx, sy, sz = map(float, p[7].split())
                    age = int(p[8])
                except ValueError:
                    bad += 1
                    continue
                peds[(path, p[2])].append((ms, fx, fz, px, pz, turn, trans, sx, sz, age, math.sqrt(fx * fx + fy * fy + fz * fz)))
    return peds, bad


def pct(v, q):
    if not v:
        return float("nan")
    s = sorted(v)
    return s[min(len(s) - 1, int(q * len(s)))]


def yaw(x, z):
    return math.degrees(math.atan2(x, z))


def wrap(d):
    return (d + 180.0) % 360.0 - 180.0


def main(paths):
    peds, bad = load(paths)
    n = sum(len(v) for v in peds.values())
    print(f"peds {len(peds)}, lines {n}, malformed {bad}")
    lengths = [s[10] for v in peds.values() for s in v]
    print("facing length p1 %.3f p50 %.3f p99 %.3f" % (pct(lengths, 0.01), pct(lengths, 0.5), pct(lengths, 0.99)))
    rates = defaultdict(list)
    by_angle = defaultdict(list)
    speeds = []
    for samples in peds.values():
        samples.sort()
        for a, b in zip(samples, samples[1:]):
            dt = (b[0] - a[0]) / 1000.0
            if not 0.04 <= dt <= 0.4:
                continue
            speed = math.hypot(b[3] - a[3], b[4] - a[4]) / dt
            speeds.append(speed)
            rate = wrap(yaw(b[1], b[2]) - yaw(a[1], a[2])) / dt
            moving = speed > 0.5
            if a[5] == 0 or b[5] == 0:
                rates["turn clip"].append(abs(rate))
                continue
            if a[6] == 0 or b[6] == 0:
                rates["transition"].append(abs(rate))
                continue
            rates["steering, moving" if moving else "steering, standing"].append(abs(rate))
            steer_ok = a[9] >= 0 and a[9] < 400 and math.hypot(a[7], a[8]) > 0.5
            if moving and steer_ok:
                gap = wrap(yaw(a[7], a[8]) - yaw(a[1], a[2]))
                # Rate toward the steering direction (positive = closing the gap).
                toward = rate * (1 if gap > 0 else -1)
                by_angle[min(int(abs(gap) // 5) * 5, 60)].append(toward)
    print("speed m/s p50 %.2f p90 %.2f" % (pct(speeds, 0.5), pct(speeds, 0.9)))
    for k, v in sorted(rates.items()):
        print(f"{k}: n={len(v)} yaw deg/s p50 %.1f p75 %.1f p90 %.1f p95 %.1f p99 %.1f max %.1f" % (
            pct(v, 0.5), pct(v, 0.75), pct(v, 0.9), pct(v, 0.95), pct(v, 0.99), max(v) if v else float("nan")))
    print("steering, moving: yaw rate toward the steering direction by facing-to-steering angle")
    for k in sorted(by_angle):
        v = by_angle[k]
        label = f"{k}-{k + 5}" if k < 60 else "60+"
        print(f"  {label:>6} deg: n={len(v):5d} p25 %6.1f p50 %6.1f p75 %6.1f p90 %6.1f" % (pct(v, 0.25), pct(v, 0.5), pct(v, 0.75), pct(v, 0.9)))


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    main(sys.argv[1:])
