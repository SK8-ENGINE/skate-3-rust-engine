"""Summarise e2e_bench.sh timing: per run dir, the game-thread call and render-block times pooled
over every scenario (p50 / p90 / p99 / p99.9 / max, µs, and the total), side by side for each
run given.

    py -3.13 .claude/skills/optimisation/tools/timing_summary.py RUN_DIR [RUN_DIR...]
"""
import sys
from pathlib import Path


def load(d: Path):
    out = {}
    for sub in sorted(p for p in d.iterdir() if p.is_dir()):
        kinds = {'frame': [], 'block': []}
        for f in sub.glob('*.ours.timing.tsv'):
            for line in f.read_text().splitlines()[1:]:
                k, us = line.split('\t')
                kinds[k].append(float(us))
        out[sub.name] = kinds
    return out


def q(v, p):
    return v[min(len(v) - 1, int((len(v) - 1) * p))]


def main():
    runs = [(Path(a).name, load(Path(a))) for a in sys.argv[1:]]
    sets = sorted({s for _, r in runs for s in r})
    for s in sets:
        for kind in ('block', 'frame'):
            print(f'{s} {kind}:')
            for name, r in runs:
                v = sorted(r.get(s, {}).get(kind, []))
                if not v:
                    continue
                print(f'  {name:>16}: n {len(v):6d}  p50 {q(v, .5):7.1f}  p90 {q(v, .9):7.1f}  p99 {q(v, .99):7.1f}'
                      f'  p99.9 {q(v, .999):7.1f}  max {v[-1]:8.1f}  sum {sum(v) / 1e3:9.1f} ms')


if __name__ == '__main__':
    main()
