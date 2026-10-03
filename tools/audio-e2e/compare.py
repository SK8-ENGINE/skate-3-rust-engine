"""End-to-end audio comparison: compare two headless renders of the same scenario in one folder
(`<name>.<A>.f32` and `<name>.<B>.f32`, raw f32, 6 channels L, R, C, LFE, Ls, Rs, 48 kHz, one settle
second first) per segment of the scenario (rows with the same state/air/grind/brake/manual/balance/speed
bucket form one segment). A = `ours` (the engine's e2e render); B = any other render of the same
scenario, renamed to `<name>.<B>.f32` (another build, or another audio stack such as upstream PR #4's
proof of concept).

Per segment and side: RMS (dBFS of the common fold L + 0.707*C + 0.5*Ls, no output gain),
octave-band levels 63 Hz .. 16 kHz, spectral centroid, dominant pitch (autocorrelation, 60–2000 Hz)
and the 10 ms envelope's onset count. Also the lag (ms) that best aligns the two envelopes and
their correlation (rolling "sync").

usage: py -3.13 tools/audio-e2e/compare.py [DIR] [--only a,b] [--a ours] [--b other] [--json OUT]
  DIR defaults to .local/audio-e2e.
"""
import json
import math
import sys
from pathlib import Path

import numpy as np

DIR = Path(__file__).resolve().parents[2] / '.local/audio-e2e'
RATE = 48000
FRAME = 800  # samples per 60 Hz frame
BANDS = [63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]


def load(path):
    d = np.fromfile(path, dtype='<f4')
    d = d[: len(d) // 6 * 6].reshape(-1, 6)
    left = d[:, 0] + 0.707 * d[:, 2] + 0.5 * d[:, 4]
    right = d[:, 1] + 0.707 * d[:, 2] + 0.5 * d[:, 5]
    return np.stack([left, right], 1)[RATE:]  # drop the settle second


def rows(tsv):
    lines = tsv.read_text().splitlines()
    cols = lines[0].split('\t')
    return [dict(zip(cols, map(float, l.split('\t')))) for l in lines[1:]]


def segments(rs):
    def key(r):
        return (int(r['state']), int(r['air']), int(r['grinding']), int(r['brake']), int(r['balance']),
                int(r['wheels']), round(r['speed'] * 3.6 / 5))
    out, start = [], 0
    for i in range(1, len(rs) + 1):
        if i == len(rs) or key(rs[i]) != key(rs[start]):
            out.append((start, i, rs[start]))
            start = i
    # merge tiny segments (< 0.25 s) into the previous one
    merged = []
    for s in out:
        if merged and s[1] - s[0] < 15:
            merged[-1] = (merged[-1][0], s[1], merged[-1][2])
        else:
            merged.append(s)
    return merged


def db(x):
    return 10 * math.log10(max(float(x), 1e-20))


def metrics(x):
    if len(x) < 1024:
        return None
    mono = x.mean(1)
    rms = db(np.mean(x * x))
    spec = np.abs(np.fft.rfft(mono * np.hanning(len(mono)))) ** 2
    freqs = np.fft.rfftfreq(len(mono), 1 / RATE)
    total = spec.sum() + 1e-30
    bands = []
    for f in BANDS:
        sel = (freqs >= f / math.sqrt(2)) & (freqs < f * math.sqrt(2))
        bands.append(db(spec[sel].sum() / total))
    centroid = float((freqs * spec).sum() / total)
    # pitch: autocorrelation of the mono signal (first 0.5 s max)
    seg = mono[: min(len(mono), RATE // 2)]
    seg = seg - seg.mean()
    ac = np.fft.irfft(np.abs(np.fft.rfft(seg, 2 * len(seg))) ** 2)[: len(seg)]
    lo, hi = RATE // 2000, RATE // 60
    pitch = float('nan')
    if ac[0] > 0 and hi < len(ac):
        k = lo + int(np.argmax(ac[lo:hi]))
        if ac[k] / ac[0] > 0.3:
            pitch = RATE / k
    env = np.sqrt(np.mean(x[: len(x) // 480 * 480].reshape(-1, 480, 2) ** 2, axis=(1, 2)) + 1e-20)
    e = 20 * np.log10(env)
    onsets = int(np.sum((e[1:] - e[:-1]) > 6.0))
    return dict(rms=rms, bands=bands, centroid=centroid, pitch=pitch, onsets=onsets, env=e)


def lag(ea, eb):
    n = min(len(ea), len(eb))
    if n < 20:
        return float('nan'), float('nan')
    a, b = ea[:n] - ea[:n].mean(), eb[:n] - eb[:n].mean()
    best, bk = -2, 0
    for k in range(-30, 31):  # ±300 ms in 10 ms steps
        if k >= 0:
            c = np.corrcoef(a[k:], b[: n - k])[0, 1] if n - k > 10 else 0
        else:
            c = np.corrcoef(a[: n + k], b[-k:])[0, 1] if n + k > 10 else 0
        if c > best:
            best, bk = c, k
    return bk * 10.0, float(best)


def main():
    if '-h' in sys.argv or '--help' in sys.argv:
        sys.exit(__doc__)
    d = Path(sys.argv[1]) if len(sys.argv) > 1 and not sys.argv[1].startswith('--') else DIR
    only = set(sys.argv[sys.argv.index('--only') + 1].split(',')) if '--only' in sys.argv else None
    sa = sys.argv[sys.argv.index('--a') + 1] if '--a' in sys.argv else 'ours'
    sb = sys.argv[sys.argv.index('--b') + 1] if '--b' in sys.argv else 'other'
    report = {}
    for tsv in sorted(d.glob('*.tsv')):
        name = tsv.stem
        if name.endswith('voices') or (only and name not in only):
            continue
        pa, pb = d / f'{name}.{sa}.f32', d / f'{name}.{sb}.f32'
        if not (pa.exists() and pb.exists()):
            continue
        xa, xb = load(pa), load(pb)
        rs = rows(tsv)
        print(f'\n== {name}   ({sa} vs {sb})')
        print(f'{"segment":34s} {"RMS dB":>15s}  {"d":>6s}  {"centroid Hz":>13s}  {"pitch Hz":>13s}  onsets  lag ms/corr   octave bands d (63..16k, dB rel. total)')
        report[name] = []
        for a, b, r in segments(rs):
            ma, mb = metrics(xa[a * FRAME:b * FRAME]), metrics(xb[a * FRAME:b * FRAME])
            if not ma or not mb:
                continue
            label = f'{a / 60:4.1f}-{b / 60:4.1f}s st{int(r["state"])} {r["speed"] * 3.6:3.0f}kmh' + \
                    ('' if not r['grinding'] else ' grind') + (' brake' if r['brake'] else '') + (' manual' if r['balance'] else '') + \
                    (' air' if r['air'] else '')
            lg, corr = lag(ma['env'], mb['env'])
            bd = ' '.join(f'{x - y:+5.1f}' for x, y in zip(ma['bands'], mb['bands']))
            print(f'{label:34s} {ma["rms"]:6.1f} / {mb["rms"]:6.1f}  {ma["rms"] - mb["rms"]:+6.1f}  {ma["centroid"]:5.0f} / {mb["centroid"]:5.0f}  '
                  f'{ma["pitch"]:5.0f} / {mb["pitch"]:5.0f}  {ma["onsets"]:3d}/{mb["onsets"]:3d}  {lg:+5.0f}/{corr:4.2f}   {bd}')
            report[name].append(dict(segment=label, rms=[ma['rms'], mb['rms']], centroid=[ma['centroid'], mb['centroid']],
                                     pitch=[ma['pitch'], mb['pitch']], onsets=[ma['onsets'], mb['onsets']], lag=lg, corr=corr,
                                     bands=[ma['bands'], mb['bands']]))
    if '--json' in sys.argv:
        Path(sys.argv[sys.argv.index('--json') + 1]).write_text(json.dumps(report, indent=1))


if __name__ == '__main__':
    main()
