"""Tire-scrub loop: long aperiodic noise bed + quiet source texture."""
from __future__ import annotations

import array
import math
import random
import statistics
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT / "_brake_source.wav"
OUT = ROOT / "brake.wav"
SR_OUT = 48000
LOOP_S = 8.0
TARGET_RMS = 0.24


def load_mono(path: Path) -> tuple[int, list[float]]:
    with wave.open(str(path), "rb") as w:
        sr = w.getframerate()
        ch = w.getnchannels()
        samp = array.array("h")
        samp.frombytes(w.readframes(w.getnframes()))
    mono: list[float] = []
    for i in range(0, len(samp), ch):
        v = samp[i]
        if ch == 2:
            v = (samp[i] + samp[i + 1]) // 2
        mono.append(v / 32768.0)
    return sr, mono


def resample(mono: list[float], sr_in: int, sr_out: int) -> list[float]:
    if sr_in == sr_out:
        return mono
    ratio = sr_out / sr_in
    n_out = int(len(mono) * ratio)
    out: list[float] = []
    for i in range(n_out):
        src_i = i / ratio
        i_lo = int(math.floor(src_i))
        i_hi = min(i_lo + 1, len(mono) - 1)
        f = src_i - i_lo
        out.append(mono[i_lo] * (1.0 - f) + mono[i_hi] * f)
    return out


def one_pole_coeff(fc: float, sr: int) -> float:
    return math.exp(-2.0 * math.pi * fc / sr)


def highpass(mono: list[float], fc: float, sr: int) -> list[float]:
    a = one_pole_coeff(fc, sr)
    y = 0.0
    prev = 0.0
    out: list[float] = []
    for v in mono:
        y = a * (y + v - prev)
        prev = v
        out.append(y)
    return out


def lowpass(mono: list[float], fc: float, sr: int) -> list[float]:
    a = one_pole_coeff(fc, sr)
    y = 0.0
    out: list[float] = []
    for v in mono:
        y = a * y + (1.0 - a) * v
        out.append(y)
    return out


def bandpass(mono: list[float], lo: float, hi: float, sr: int) -> list[float]:
    return lowpass(highpass(mono, lo, sr), hi, sr)


def rms(mono: list[float]) -> float:
    return math.sqrt(sum(x * x for x in mono) / max(len(mono), 1))


def best_stationary_window(mono: list[float], sr: int) -> tuple[int, int]:
    win = int(sr * 0.36)
    hop = int(sr * 0.02)
    best_score = -1e9
    best_i = 0
    for i in range(0, len(mono) - win, hop):
        seg = mono[i : i + win]
        sub = hop * 2
        subs = [
            math.sqrt(sum(x * x for x in seg[j : j + sub]) / sub)
            for j in range(0, len(seg) - sub, sub)
        ]
        var = statistics.variance(subs) if len(subs) > 1 else 0.0
        level = rms(seg)
        score = level * 2.0 - var * 12.0 - abs(level - 0.21) * 4.0
        if score > best_score:
            best_score = score
            best_i = i
    return best_i, win


def hann(n: int) -> list[float]:
    if n <= 1:
        return [1.0] * max(n, 0)
    return [0.5 - 0.5 * math.cos(2.0 * math.pi * i / (n - 1)) for i in range(n)]


def pink_noise(n: int, rng: random.Random) -> list[float]:
    rows = 7
    state = [rng.uniform(-1.0, 1.0) for _ in range(rows)]
    out: list[float] = []
    counter = 0
    for _ in range(n):
        changed = counter ^ (counter - 1)
        counter += 1
        for i in range(rows):
            if changed & (1 << i):
                state[i] = rng.uniform(-1.0, 1.0)
        out.append(sum(state) / rows)
    return out


def slow_envelope(n: int, sr: int, rng: random.Random) -> list[float]:
    """Irregular 0.35..1.0 gain so the bed never sits at one level."""
    env = [1.0] * n
    pos = 0
    while pos < n:
        span = int(sr * rng.uniform(0.18, 0.55))
        target = rng.uniform(0.38, 1.0)
        for j in range(span):
            idx = pos + j
            if idx >= n:
                break
            t = j / max(span - 1, 1)
            smooth = 0.5 - 0.5 * math.cos(math.pi * t)
            env[idx] = env[idx - 1] * (1.0 - smooth) + target * smooth if idx > 0 else target
        pos += span
    return env


def add_grain(
    out: list[float],
    pool: list[float],
    pos: int,
    grain_len: int,
    window: list[float],
    gain: float,
    start: int,
) -> None:
    n_loop = len(out)
    for j in range(grain_len):
        idx = (pos + j) % n_loop
        out[idx] += pool[start + j] * window[j] * gain


def texture_layer(pool: list[float], sr: int, seconds: float, rng: random.Random) -> list[float]:
    n_loop = int(sr * seconds)
    out = [0.0] * n_loop
    grain_ms_lo, grain_ms_hi = 68, 118
    hop_ms_lo, hop_ms_hi = 24, 61
    pos = rng.randint(0, n_loop // 4)
    while pos < n_loop:
        grain_len = max(48, int(sr * rng.uniform(grain_ms_lo, grain_ms_hi) / 1000.0))
        hop = max(20, int(sr * rng.uniform(hop_ms_lo, hop_ms_hi) / 1000.0))
        if len(pool) <= grain_len + 2:
            break
        start = rng.randint(0, len(pool) - grain_len - 1)
        add_grain(out, pool, pos, grain_len, hann(grain_len), rng.uniform(0.55, 1.0), start)
        pos += hop
    return out


def normalize_rms(mono: list[float], target: float) -> list[float]:
    cur = rms(mono)
    if cur < 1e-8:
        return mono
    scale = target / cur
    return [max(-1.0, min(1.0, x * scale)) for x in mono]


def seam_crossfade(mono: list[float], sr: int, fade_ms: float = 90.0) -> list[float]:
    n = len(mono)
    fade = max(16, int(sr * fade_ms / 1000.0))
    out = mono[:]
    for i in range(fade):
        t = i / max(fade - 1, 1)
        w = 0.5 - 0.5 * math.cos(math.pi * t)
        tail = out[n - fade + i]
        head = out[i]
        blended = tail * (1.0 - w) + head * w
        out[i] = blended
        out[n - fade + i] = blended
    return out


def seam_delta(mono: list[float]) -> float:
    n = min(512, len(mono))
    if n <= 1:
        return 0.0
    return sum(abs(mono[i] - mono[i - len(mono)]) for i in range(n)) / n


def build_loop(source: Path) -> tuple[list[float], dict[str, float | str]]:
    sr_in, mono = load_mono(source)
    i0, win = best_stationary_window(mono, sr_in)
    pool = mono[i0 : i0 + win]
    pool = bandpass(pool, 320.0, 4200.0, sr_in)
    pool = resample(pool, sr_in, SR_OUT)

    rng = random.Random(90211)
    n_loop = int(SR_OUT * LOOP_S)

    noise = pink_noise(n_loop, rng)
    noise = bandpass(noise, 280.0, 3600.0, SR_OUT)
    env = slow_envelope(n_loop, SR_OUT, rng)
    noise = [noise[i] * env[i] for i in range(n_loop)]

    texture = texture_layer(pool, SR_OUT, LOOP_S, rng)
    texture = bandpass(texture, 350.0, 3200.0, SR_OUT)

    loop = [noise[i] * 0.48 + texture[i] * 0.52 for i in range(n_loop)]
    loop = normalize_rms(loop, TARGET_RMS)
    loop = seam_crossfade(loop, SR_OUT)

    peak = max(abs(x) for x in loop) or 1.0
    if peak > 0.94:
        scale = 0.92 / peak
        loop = [x * scale for x in loop]

    meta: dict[str, float | str] = {
        "method": "noise_texture_blend",
        "source_start_s": i0 / sr_in,
        "source_span_s": win / sr_in,
        "loop_s": LOOP_S,
        "sr": float(SR_OUT),
        "rms": rms(loop),
        "seam_delta": seam_delta(loop),
        "noise_share": 0.48,
        "texture_share": 0.52,
    }
    return loop, meta


def write_stereo(path: Path, mono: list[float], sr: int) -> None:
    pcm = array.array("h")
    for x in mono:
        s = int(max(-32768, min(32767, round(x * 32767))))
        pcm.append(s)
        pcm.append(s)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(pcm.tobytes())


def main() -> None:
    source = SOURCE if SOURCE.is_file() else OUT
    if not source.is_file():
        raise SystemExit(f"Missing source wav: {source}")
    loop, meta = build_loop(source)
    write_stereo(OUT, loop, SR_OUT)
    print(f"Wrote {OUT.name}: {meta}")


if __name__ == "__main__":
    main()
