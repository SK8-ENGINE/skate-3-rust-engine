# Stream a Chrome trace (profiling.rs capture), pick N fixed steps late in it and show what fills each step on its
# thread: span names by time, and the uncovered time between leaf spans (USER-ASKS 330).
# Usage: py -3.13 -I tick_gaps.py <trace.json> [steps=20]
import json, re, sys, collections
path = sys.argv[1]; want = int(sys.argv[2]) if len(sys.argv) > 2 else 20
ts_re = re.compile(rb'"ts":(\d+)'); dur_re = re.compile(rb'"dur":(\d+)'); tid_re = re.compile(rb'"tid":(\d+)')
MARK = b'"name":"schedule name=FixedMain"'
steps = []
with open(path, 'rb') as f:
    for line in f:
        if MARK in line:
            steps.append((int(ts_re.search(line).group(1)), int(dur_re.search(line).group(1)), int(tid_re.search(line).group(1))))
print('steps', len(steps))
pick = steps[-(want + 60):-60]
lo, hi = pick[0][0], pick[-1][0] + pick[-1][1]
events = []
with open(path, 'rb') as f:
    for line in f:
        m = ts_re.search(line)
        if not m or b'"dur"' not in line: continue
        ts = int(m.group(1))
        if ts < lo or ts > hi: continue
        events.append(json.loads(line.rstrip(b'\r\n').rstrip(b',')))
names = collections.Counter(); counts = collections.Counter(); other_threads = collections.Counter()
gap_total = 0; step_total = 0; gaps = collections.Counter()
def is_leaf(n):
    return (n.startswith('system name=') and 'run_fixed_main' not in n) or n.startswith('system_commands')
for (s, d, tid) in pick:
    step_total += d
    inside = [e for e in events if e['ts'] >= s and e['ts'] + e['dur'] <= s + d]
    for e in inside:
        if e['tid'] == tid: names[e['name']] += e['dur']; counts[e['name']] += 1
        else: other_threads[e['name']] += e['dur']
    leaves = sorted([e for e in inside if is_leaf(e['name'])], key=lambda e: e['ts'])
    cur = s; prev = 'START'
    for e in leaves:
        g = e['ts'] - cur
        if g > 0:
            # what non-leaf spans on the step thread cover this gap
            cover = [x['name'] for x in inside if x['tid'] == tid and not is_leaf(x['name']) and x['ts'] < e['ts'] and x['ts'] + x['dur'] > cur and not x['name'].startswith('schedule name=Fixed') and 'run_fixed_main' not in x['name']]
            gaps[(prev, e['name'], ','.join(sorted(set(cover)))[:80])] += g; gap_total += g
        cur = max(cur, e['ts'] + e['dur']); prev = e['name']
    if s + d > cur: gaps[(prev, 'END', '')] += s + d - cur; gap_total += s + d - cur
n = len(pick)
print(f'{n} steps, step {step_total/n/1000:.3f} ms, not covered by any system span {gap_total/n/1000:.3f} ms')
print('span names on the step thread (ms per step, runs per step):')
for k, v in names.most_common(30): print(f'  {v/n/1000:7.3f} {counts[k]/n:6.1f}  {k[:120]}')
print('other threads inside the steps (ms per step):')
for k, v in other_threads.most_common(8): print(f'  {v/n/1000:7.3f}  {k[:120]}')
print('largest gaps (us per step): prev -> next [covering spans]')
for k, v in gaps.most_common(20): print(f'  {v/n:7.1f}  {k[0][13:70]} -> {k[1][13:70]} [{k[2]}]')
# Per-step dispatch count: leaf spans per step and the systems that ran but did almost nothing (<= 2 us).
leaf_runs = collections.Counter(); tiny = collections.Counter()
for (s, d, tid) in pick:
    for e in events:
        if e['ts'] >= s and e['ts'] + e['dur'] <= s + d and is_leaf(e['name']):
            leaf_runs[e['name']] += 1
            if e['dur'] <= 2: tiny[e['name']] += 1
print(f'leaf spans per step {sum(leaf_runs.values())/n:.1f}, of them <= 2 us {sum(tiny.values())/n:.1f}')
for k, v in tiny.most_common(60): print(f'  {v/n:4.1f}  {k[13:110]}')
