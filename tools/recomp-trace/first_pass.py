"""Summarise first-pass raw-register hook lines (hooks_npc.cpp format) per kind.

usage: py -3.13 tools/recomp-trace/first_pass.py <trace.tsv> [KIND ...]
Line format: KIND ms [tag] skipped=N | r3 r4 r5 r6 r7 r8 | f1 f2 | ret r3 | ret f1 | callers.
For each kind (and tag), prints the call count (logged + skipped), then per field the number of distinct
values and the most common ones: constant fields are object pointers or flags, varying ones are data.
"""
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from trace import Trace  # noqa: E402

FIELDS = ['r3', 'r4', 'r5', 'r6', 'r7', 'r8', 'f1', 'f2', 'ret', 'retf', 'callers']


def rows(trace: Trace, kind: str):
    for ms, f in trace.lines.get(kind, []):
        tag = ''
        if f and not f[0].startswith('skipped='):
            tag, f = f[0], f[1:]
        if len(f) < 6 or not f[0].startswith('skipped='):
            continue  # interleaved / truncated line
        regs = f[1].split()
        floats = f[2].split()
        if len(regs) != 6 or len(floats) != 2:
            continue
        yield tag, ms, int(f[0][8:]), dict(zip(FIELDS, regs + floats + [f[3], f[4], f[5]]))


def main() -> None:
    if len(sys.argv) < 2 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    trace = Trace(sys.argv[1])
    kinds = sys.argv[2:] or sorted(k for k in trace.lines if trace.lines[k] and
                                   trace.lines[k][0][1][-1:] and '<' in trace.lines[k][0][1][-1])
    for kind in kinds:
        groups = defaultdict(list)
        for tag, ms, skipped, values in rows(trace, kind):
            groups[tag].append((ms, skipped, values))
        for tag, items in groups.items():
            total = len(items) + sum(s for _, s, _ in items)
            span = (items[-1][0] - items[0][0]) / 1000 if len(items) > 1 else 0
            print(f'== {kind} {tag} logged={len(items)} calls~{total} over {span:.0f}s')
            for field in FIELDS:
                counts = Counter(v[field] for _, _, v in items)
                top = ', '.join(f'{k}×{n}' for k, n in counts.most_common(4))
                print(f'   {field:7s} distinct={len(counts):5d}  {top}')


if __name__ == '__main__':
    main()
