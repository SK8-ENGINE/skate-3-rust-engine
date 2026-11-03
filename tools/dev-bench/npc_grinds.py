"""Per recorded grind in a session log: was the grind caught? usage: py -I npc_grinds.py <stderr.log> [window_s]"""
import re, sys, collections
from datetime import datetime

ANSI = re.compile(r"\x1b\[[0-9;]*m")
SLOT = re.compile(r"NPC_SKATER_SIM_SLOT #(\d+) (\S+) category 5 (\S+) node (\d+)")
ENTRY = re.compile(r"NPC_GRIND_ENTRY requested=(\S+) .*?cursor_node=(\d+)")
LAND = re.compile(r"NPC_SKATER_LAND #(\d+) \S+ node (\d+) flip (\S+) char\.node (\S+)")
BAIL = re.compile(r"NPC_SKATER_BAIL #(\d+) .*?from Some\((\w+)\).*?node (\d+)")
OFF = re.compile(r"NPC_SKATER_PATH #(\d+) \S+ left the path at node (\d+) \(A \S+, ([\d.]+) m")

window = float(sys.argv[2]) if len(sys.argv) > 2 else 0.5
slots, entries, lands, bails, offs = [], [], [], [], []
for raw in open(sys.argv[1], encoding="utf-8", errors="replace"):
    line = ANSI.sub("", raw)
    if "NPC_" not in line or not line.startswith("20"):
        continue
    t = datetime.fromisoformat(line[:26]).timestamp()
    if m := SLOT.search(line):
        slots.append((t, line[11:19], m[1], m[2], m[3], int(m[4])))
    elif m := ENTRY.search(line):
        entries.append((t, m[1], int(m[2])))
    elif m := LAND.search(line):
        lands.append((t, m[1], int(m[2]), m[3], float(m[4])))
    elif m := BAIL.search(line):
        bails.append((t, m[1], m[2], int(m[3])))
    elif m := OFF.search(line):
        offs.append((t, m[1], int(m[2]), float(m[3])))

totals = collections.Counter()
by_flip = collections.Counter()
by_node = collections.defaultdict(collections.Counter)
rows = []
for t, clock, npc, name, trick, node in slots:
    grind = [e for e in entries if abs(e[0] - t) <= window and node - 3 <= e[2] <= node + 1 and e[1] != "SlideGround"]
    land = next((l for l in lands if l[1] == npc and abs(l[0] - t) <= window), None)
    bail = next((b for b in bails if b[1] == npc and -window <= b[0] - t <= 1.5), None)
    off = next((o for o in offs if o[1] == npc and 0 <= t - o[0] <= 3.0), None)
    result = f"caught {grind[0][1]}" if grind else ("BAIL" if bail else "MISSED")
    kind = "caught" if grind else ("bail" if bail else "missed")
    totals[kind] += 1
    by_node[(trick, node)][kind] += 1
    if land:
        by_flip[(land[3], kind)] += 1
    rows.append(f"{clock} #{npc} {name:18} {trick:18} node {node:3} {result:24} land {land[3:] if land else '-'} off {off[3] if off else '-'}")

print("\n".join(rows))
print("\ntotals", dict(totals))
print("by landing flip", dict(by_flip))
print("worst lines:")
for key, c in sorted(by_node.items(), key=lambda kv: -(kv[1]["missed"] + kv[1]["bail"]))[:12]:
    print(" ", key, dict(c))
print("NPC ids reuse a line per character; a trick name with node identifies one recorded grind only within a line.")
