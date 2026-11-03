"""Finds where the skater catches on ground (sudden speed loss, wheel contacts dropping, bails) in a trace's
audio_state.tsv. Used for the skate.Park seam reports (doc 39, USER-ASKS 221).

usage: python park-catches.py <audio_state.tsv>                 -> segments (split at board jumps > 20 m)
       python park-catches.py <audio_state.tsv> <run> <from_ms> <to_ms> -> catches in that window of map run <run>
       (a run = rows until ms goes backwards, i.e. one map load; numbered from 0 in the segment listing)
"""
import csv
import math
import sys

rows = []
with open(sys.argv[1], newline='') as f:
    for row in csv.DictReader((line for line in f if not line.startswith('#')), delimiter='\t'):
        try:
            rows.append(row | {'t': float(row['ms']), 'v': float(row['speed']),
                               'p': (float(row['board_x']), float(row['board_y']), float(row['board_z']))})
        except (ValueError, KeyError):
            continue

runs, run = [], -1
for i, r in enumerate(rows):
    if i == 0 or r['t'] < rows[i - 1]['t']:
        run += 1
    r['run'] = run

if len(sys.argv) < 5:
    start = 0
    for i in range(1, len(rows) + 1):
        if i == len(rows) or math.dist(rows[i]['p'], rows[i - 1]['p']) > 20:
            a, b = rows[start], rows[i - 1]
            bails = sum(1 for j in range(start + 1, i) if rows[j]['bail'] not in ('0', '') and rows[j - 1]['bail'] in ('0', ''))
            print(f"run {a['run']:>2} {a['t']:>9.0f}-{b['t']:>9.0f} ms  {(b['t'] - a['t']) / 1000:6.1f} s  start "
                  f"({a['p'][0]:.0f}, {a['p'][1]:.0f}, {a['p'][2]:.0f})  bails {bails}")
            start = i
    sys.exit()

run, lo, hi = int(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
window = [r for r in rows if r['run'] == run and lo <= r['t'] <= hi]
for i in range(1, len(window)):
    r, prev = window[i], window[i - 1]
    back = next((w for w in reversed(window[:i]) if r['t'] - w['t'] >= 100), window[0])
    drop = back['v'] - r['v']
    bail = r['bail'] not in ('0', '') and prev['bail'] in ('0', '')
    lost = prev['wheels'] != r['wheels'] and r['air'] == '0'
    if bail or (r['air'] == '0' and back['air'] == '0' and drop > 2.0 and back['v'] > 3.0):
        print(f"{r['t']:>9.0f} {'BAIL ' if bail else 'DROP '} v {back['v']:5.2f}->{r['v']:5.2f} wheels {prev['wheels']}->{r['wheels']} "
              f"state {r['state']} at ({r['p'][0]:.2f}, {r['p'][1]:.2f}, {r['p'][2]:.2f}) heading {r['heading']} "
              f"seam {r['seam0']}/{r['seam3']} tag {r['tag']} slope {r['slope']}")
    elif lost and r['v'] > 3.0:
        print(f"{r['t']:>9.0f} WHEEL v {r['v']:5.2f} wheels {prev['wheels']}->{r['wheels']} at "
              f"({r['p'][0]:.2f}, {r['p'][1]:.2f}, {r['p'][2]:.2f}) seam {r['seam0']}/{r['seam3']}")
