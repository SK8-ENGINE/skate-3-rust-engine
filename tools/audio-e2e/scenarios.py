"""End-to-end audio, step 1: write the scripted per-frame player situations the headless render plays
(`cargo test -p skate-game --release --bin skate3rust -- --ignored e2e_render`, with E2E_DIR=OUT).
Any other audio stack that reads the same TSVs can render them too, for compare.py. One TSV per
scenario in OUT (default .local/audio-e2e/), one row per 60 Hz frame, columns:

  frame speed turn wheels(mask, bit i = wheel i down) tag air air_time to_land jump_height jv
  grinding family grind_tag brake manual balance scorable push slope tilt slip feet state

speed m/s along +x; tag = the wheels' 7-bit audio surface tag; air = in known air; jv = jump
velocity delta (m/s, Air+112 y, written while the air flag 440 holds); scorable = EScorableID (-1
none, 128 ollie); state = the physical state id (100 ground, 201 known air, 400 grind, 101 slide).
usage: py -3.13 tools/audio-e2e/scenarios.py [OUT]
       py -3.13 tools/audio-e2e/scenarios.py --from-log LOG --cut A-B NAME [OUT]
  --from-log: cut a real-play log (`SKATE_AUDIO_STATE_LOG=<path>` while playing; same columns plus ms
  and positions) to a scenario: A-B in seconds of the log's own clock (column ms / 1000); frames
  renumbered from 0.
  --cut rA-B NAME: rows A..B of the log instead (its clock restarts at map loads).
"""
import math
import sys
from pathlib import Path

OUT = Path(sys.argv[1]) if len(sys.argv) > 1 and not sys.argv[1].startswith('--') else Path(__file__).resolve().parents[2] / '.local/audio-e2e'
COLS = ['frame', 'speed', 'turn', 'wheels', 'tag', 'air', 'air_time', 'to_land', 'jump_height', 'jv', 'grinding',
        'family', 'grind_tag', 'brake', 'manual', 'balance', 'scorable', 'push', 'slope', 'tilt', 'slip', 'feet', 'state']
CONCRETE = 3   # concrete (rolling grain asphalt/concrete family; both stacks resolve the tag themselves)
METAL = 9
LEDGE = 3


def row(**kw):
    r = dict(speed=0.0, turn=0.0, wheels=15, tag=CONCRETE, air=0, air_time=0.0, to_land=0.0, jump_height=0.0, jv=0.0,
             grinding=0, family=0, grind_tag=0, brake=0, manual=0, balance=0, scorable=-1, push=0, slope=0.0,
             tilt=0.0, slip=0.0, feet=3, state=100)
    r.update(kw)
    return r


def ramp(frames, a, b):
    return [a + (b - a) * i / max(1, frames - 1) for i in range(frames)]


def scenarios():
    out = {}
    for kmh in (10, 20, 30, 45):
        out[f'roll{kmh}'] = [row(speed=kmh / 3.6) for _ in range(6 * 60)]
    # Carve at 20 km/h: turn input sine ±0.8, period 2 s, after 1 s straight.
    out['carve20'] = [row(speed=20 / 3.6, turn=0.0 if f < 60 else 0.8 * math.sin(2 * math.pi * (f - 60) / 120)) for f in range(6 * 60)]
    # Speed sweep 0 → 40 km/h over 6 s then back (rolling sync / pitch tracking).
    sweep = ramp(360, 0.0, 40 / 3.6) + ramp(240, 40 / 3.6, 0.0)
    out['sweep'] = [row(speed=v) for v in sweep]
    # Ollie at 20 km/h: 1.5 s roll, takeoff (ollie scorable, jump velocity), 0.7 s air, land, 2 s roll.
    seq = [row(speed=20 / 3.6) for _ in range(90)]
    air = 42
    for i in range(air):
        t = i / 60
        seq.append(row(speed=20 / 3.6, wheels=0, air=1, air_time=t, to_land=max(0.0, air / 60 - t), jump_height=0.6,
                       jv=3.2 if i == 0 else 0.0, scorable=128, state=201))
    seq += [row(speed=20 / 3.6) for _ in range(120)]
    out['ollie20'] = seq
    # Rail grind (metal, family 1 = 50-50) and ledge grind (concrete, family 0) at 6 m/s for 2.5 s.
    for name, tag, fam in (('grind_metal', METAL, 1), ('grind_ledge', LEDGE, 0)):
        seq = [row(speed=6.0) for _ in range(60)]
        seq += [row(speed=6.0, wheels=0, grinding=1, family=fam, grind_tag=tag, state=400) for _ in range(150)]
        seq += [row(speed=6.0) for _ in range(60)]
        out[name] = seq
    # Manual at 15 km/h: back wheels only, balance flag.
    seq = [row(speed=15 / 3.6) for _ in range(60)]
    seq += [row(speed=15 / 3.6, wheels=0b1100, balance=1, state=100) for _ in range(150)]
    seq += [row(speed=15 / 3.6) for _ in range(60)]
    out['manual15'] = seq
    # Foot brake from 20 km/h to 2 km/h over 2.5 s.
    seq = [row(speed=20 / 3.6) for _ in range(60)]
    seq += [row(speed=v, brake=1) for v in ramp(150, 20 / 3.6, 2 / 3.6)]
    seq += [row(speed=2 / 3.6) for _ in range(30)]
    out['brake20'] = seq
    # Powerslide at 20 km/h: lateral slip, tilt, slide state.
    seq = [row(speed=20 / 3.6) for _ in range(60)]
    seq += [row(speed=v, slip=0.8, tilt=0.25, state=101) for v in ramp(90, 20 / 3.6, 8 / 3.6)]
    seq += [row(speed=8 / 3.6) for _ in range(60)]
    out['slide20'] = seq
    return out


# Optional real-play columns (state_log.rs): copied when the log has them.
EXTRA = ['grind_impact', 'deck_impact', 'deck_tag', 'foot_y0', 'foot_y1', 'foot_xz0', 'foot_xz1',
         'seam0', 'seam3', 'wheel_x', 'wheel_z', 'heading', 'lines', 'foot_down', 'foot_tag_a', 'foot_tag_b', 'hands',
         'strength', 'foot_vy_a', 'foot_vy_b', 'step', 'body', 'limb', 'slide', 'deck_up', 'deck_contact',
         # the push plant, flags, deck spin, body regions.
         'plant', 'stroke', 'deck_spin', 'spin_x', 'spin_y', 'bail', 'bail_end', 'held', 'offboard_air', 'footplant',
         'revert', 'soft', 'face'] + [f'{k}{i}' for k in ('rimp', 'rslide', 'rtag') for i in range(6)] + [
         # the bridge's speed graph input: |COM v| (logs since 2026-10-03), else the COM positions.
         'com_speed', 'com_x', 'com_y', 'com_z']


def from_log(log, cut, name, out):
    lines = Path(log).read_text().splitlines()
    cols = lines[0].split('\t')
    rows = [dict(zip(cols, l.split('\t'))) for l in lines[1:] if l.count('\t') == len(cols) - 1]
    if cut.startswith('r'):
        # rA-B: rows A..B (0-based, inclusive), for logs whose clock restarts at map loads.
        a, b = (int(x) for x in cut[1:].split('-'))
        rows = rows[a:b + 1]
    else:
        a, b = (float(x) for x in cut.split('-'))
        t0 = float(rows[0]['ms'])
        rows = [r for r in rows if a <= (float(r['ms']) - t0) / 1000 <= b]
    out.mkdir(parents=True, exist_ok=True)
    cols_out = COLS + [c for c in EXTRA if c in cols]
    with (out / f'{name}.tsv').open('w', newline='\n') as f:
        f.write('\t'.join(cols_out) + '\n')
        for i, r in enumerate(rows):
            r['frame'] = str(i)
            f.write('\t'.join(r[c] for c in cols_out) + '\n')
    print(name, len(rows), 'frames from', log)


def main():
    if '-h' in sys.argv or '--help' in sys.argv:
        sys.exit(__doc__)
    if '--from-log' in sys.argv:
        i = sys.argv.index('--from-log')
        log, cut, name = sys.argv[i + 1], sys.argv[sys.argv.index('--cut') + 1], sys.argv[sys.argv.index('--cut') + 2]
        rest = [x for x in sys.argv[1:] if x not in (log, cut, name, '--from-log', '--cut')]
        from_log(log, cut, name, Path(rest[0]) if rest else OUT)
        return
    OUT.mkdir(parents=True, exist_ok=True)
    for name, rows in scenarios().items():
        with (OUT / f'{name}.tsv').open('w', newline='\n') as f:
            f.write('\t'.join(COLS) + '\n')
            for i, r in enumerate(rows):
                r['frame'] = i
                f.write('\t'.join(f'{r[c]:.6f}' if isinstance(r[c], float) else str(r[c]) for c in COLS) + '\n')
        print(name, len(rows), 'frames')


if __name__ == '__main__':
    main()
