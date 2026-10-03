"""Summarise retail per-channel send gains from a recomp audio trace (SEND lines).

SEND <ms> <owner> <object> <target> <channels> <cell0,cell1,...>  (send hook; change-only)
The cells are the send's per-channel gains (pan matrix row x target). For the voice graph's final
6-channel send, cells/target is the panner output for that voice, so this checks a pan law:
power sum, LFE share, most common shapes.

Usage: py -3.13 tools/recomp-trace/send_vectors.py <trace.tsv> [channels=6] [min_target=0.05]
"""
import collections
import sys


def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    path = sys.argv[1]
    want = int(sys.argv[2]) if len(sys.argv) > 2 else 6
    floor = float(sys.argv[3]) if len(sys.argv) > 3 else 0.05
    sums, lfe, shapes, peaks = [], [], collections.Counter(), []
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            if not line.startswith("SEND\t"):
                continue
            f = line.rstrip("\n").split("\t")
            if len(f) < 7 or not f[5].isdigit() or int(f[5]) != want:
                continue
            try:
                target = float(f[4])
                cells = [float(x) for x in f[6].split(",")]
            except ValueError:
                continue
            if target < floor or len(cells) != want:
                continue
            r = [c / target for c in cells]
            sums.append(sum(x * x for x in r))
            peaks.append(max(r))
            if want == 6:
                lfe.append(r[3])
            shapes[tuple(round(x, 3) for x in r)] += 1

    def pct(xs, p):
        xs = sorted(xs)
        return xs[min(len(xs) - 1, int(p * len(xs)))] if xs else float("nan")

    print(f"vectors {len(sums)} (channels {want}, target >= {floor})")
    print("sum of squares p1/p50/p99: %.4f %.4f %.4f" % (pct(sums, .01), pct(sums, .5), pct(sums, .99)))
    print("max cell p1/p50/p99: %.4f %.4f %.4f" % (pct(peaks, .01), pct(peaks, .5), pct(peaks, .99)))
    if lfe:
        print("LFE share p50/p99/max: %.4f %.4f %.4f" % (pct(lfe, .5), pct(lfe, .99), max(lfe)))
    print("most common shapes (cells / target):")
    for shape, n in shapes.most_common(30):
        print(f"  {n:6d}  {shape}")


if __name__ == "__main__":
    main()
