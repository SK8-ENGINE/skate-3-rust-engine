"""Summarise the AUDIO_TIMING lines of real play (SKATE_AUDIO_TIMING=1) per audio system: lines, the average
of the per-second averages, the per-second maximum (median / p90 / worst) and calls per second; then the render's
load per block (`block_voices` / `block_instances` / `block_grains`: mixer voices, AEMS instances, grain voices).

    py -3.13 .claude/skills/optimisation/tools/audio_timing_summary.py logs/game-YYYYMMDD-HHMMSS.stderr.log [...]
"""
import re
import statistics
import sys

PAT = re.compile(r'(\w+)=(\d+)/(\d+)us×(\d+)')
GAUGE = re.compile(r'(\w+)=(\d+)/(\d+\.\d)×(\d+)')


def main():
    for f in sys.argv[1:]:
        stats, gauges = {}, {}
        for line in open(f, encoding='utf-8', errors='replace'):
            if 'AUDIO_TIMING' not in line:
                continue
            for name, mx, avg, n in PAT.findall(line):
                stats.setdefault(name, []).append((int(mx), int(avg), int(n)))
            for name, mx, avg, n in GAUGE.findall(line):
                gauges.setdefault(name, []).append((int(mx), float(avg), int(n)))
        print(f)
        for k, v in stats.items():
            mx = sorted(x[0] for x in v)
            print(f'  {k:16s} lines {len(v):4d}  avg {statistics.mean(x[1] for x in v):8.1f} us  per-second max: median '
                  f'{mx[len(mx) // 2]:6d} p90 {mx[int(len(mx) * .9)]:6d} worst {mx[-1]:7d}  calls/s {statistics.mean(x[2] for x in v):7.1f}')
        for k, v in gauges.items():
            mx = sorted(x[0] for x in v)
            print(f'  {k:16s} lines {len(v):4d}  per block avg {statistics.mean(x[1] for x in v):6.1f}  per-second max: median '
                  f'{mx[len(mx) // 2]:4d} p90 {mx[int(len(mx) * .9)]:4d} worst {mx[-1]:4d}')


if __name__ == '__main__':
    main()
