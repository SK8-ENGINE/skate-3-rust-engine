"""How a world-emitter bank's voices follow each other in a recomp session: every PLAY of the bank
(resolved through the session's play_resolve.json cache, written by retail_voices.py), its player,
sample slot, pitch (MOD PITCH v3) and wall-clock end (sample seconds / pitch), the gap to the
previous start, the overlap with the voice before, back-to-back repeats of a slot, and the most
voices of the bank sounding at once. For "doubling / restarting" listening reports.

usage: py -3.13 tools/recomp-trace/retail_relay.py <session dir> <bank> [--quiet] [--from MS] [--to MS] [--assets DIR]
--from/--to keep one emitter run (e.g. a stretch where the player stood still). The summary also
splits the starts by relay turn (1st, 3rd, … vs 2nd, 4th, …) = the two players' shuffle bags.
Sample lengths come from the install's decoded WAVs (<assets>/private/audio/banks/<bank>/; --assets,
default the repo's assets junction).
Reference reading of our own traces; nothing is shipped.
"""
import json
import sys
import wave
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def main():
    if len(sys.argv) < 3 or sys.argv[1] in ('-h', '--help'):
        sys.exit(__doc__)
    session, bank = Path(sys.argv[1]), sys.argv[2]
    quiet = '--quiet' in sys.argv
    arg = lambda k, d: float(sys.argv[sys.argv.index(k) + 1]) if k in sys.argv else d
    lo, hi = arg('--from', float('-inf')), arg('--to', float('inf'))
    cache = json.loads((session / 'play_resolve.json').read_text())
    seconds = {}
    assets = Path(sys.argv[sys.argv.index('--assets') + 1]) if '--assets' in sys.argv else REPO / 'assets'
    for wav in sorted((assets / 'private/audio/banks' / bank).glob('*.wav')):
        with wave.open(str(wav)) as w:
            seconds[int(wav.stem)] = w.getnframes() / w.getframerate()
    plays, pitch = [], defaultdict(list)
    for raw in (session / 'trace.tsv').open(encoding='utf-8', errors='replace'):
        f = raw.rstrip('\n').split('\t')
        if len(f) < 4:
            continue
        if f[0] == 'PLAY' and len(f) > 7:
            hit = cache.get(f[7])
            if hit and hit[0] == bank and lo <= float(f[1]) <= hi:
                plays.append((float(f[1]), f[2], int(hit[1])))
        elif f[0] == 'MOD' and f[2] == 'PITCH' and len(f) > 7:
            try:
                pitch[f[3]].append((float(f[1]), float(f[7])))
            except ValueError:
                pass
    if not plays:
        print(f'{session.name}: no {bank} plays')
        return
    rows, live, most, repeats = [], [], 0, 0
    prev = None
    for ms, player, slot in plays:
        p = [v for t, v in pitch[player] if ms - 1 <= t <= ms + 400 and v > 0]
        ratio = p[-1] if p else 1.0
        end = ms + 1000.0 * seconds.get(slot, 0.0) / max(ratio, 0.25)
        live = [e for e in live if e > ms]
        live.append(end)
        most = max(most, len(live))
        gap = ms - prev[0] if prev else float('nan')
        overlap = prev[3] - ms if prev else float('nan')
        same = prev is not None and prev[2] == slot
        repeats += same
        rows.append((ms, player, slot, ratio, end))
        if not quiet:
            print(f'{ms / 1000:9.3f}s {player} slot {slot:2d} pitch {ratio:.3f} lasts {(end - ms) / 1000:.3f}s '
                  f'gap {gap / 1000:6.3f}s overlap {overlap / 1000:6.3f}s{"  SAME SLOT" if same else ""}')
        prev = (ms, player, slot, end)
    span = (plays[-1][0] - plays[0][0]) / 1000
    players = sorted({r[1] for r in rows})
    print(f'{session.name} {bank}: {len(plays)} starts over {span:.1f} s, players {len(players)}, '
          f'most at once {most}, back-to-back same slot {repeats}')
    slots = [r[2] for r in rows]
    print(f'  by relay turn: A {slots[0::2]}  B {slots[1::2]}')


if __name__ == '__main__':
    main()
