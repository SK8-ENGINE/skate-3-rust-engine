"""Resolve conflict hunks in order: pick.py FILE CHOICE... (o, t, ot, to; 'ot:SEP' joins with SEP, \n escapes)."""
import sys, re
path, choices = sys.argv[1], sys.argv[2:]
lines = open(path, encoding="utf-8", newline="").read().splitlines(keepends=True)
out, i, h = [], 0, 0
while i < len(lines):
    l = lines[i]
    if l.startswith("<<<<<<< "):
        ours, theirs, j = [], [], i + 1
        while not lines[j].startswith(("=======", "||||||| ")): ours.append(lines[j]); j += 1
        if lines[j].startswith("||||||| "):
            while not lines[j].startswith("======="): j += 1
        j += 1
        while not lines[j].startswith(">>>>>>> "): theirs.append(lines[j]); j += 1
        c = choices[h]; h += 1
        kind, _, sep = c.partition(":")
        sep = sep.encode().decode("unicode_escape")
        parts = {"o": ours, "t": theirs}
        out += parts[kind[0]]
        if len(kind) > 1:
            if sep: out.append(sep)
            out += parts[kind[1]]
        i = j + 1
    else:
        out.append(l); i += 1
assert h == len(choices), f"{h} hunks, {len(choices)} choices"
open(path, "w", encoding="utf-8", newline="").write("".join(out))
