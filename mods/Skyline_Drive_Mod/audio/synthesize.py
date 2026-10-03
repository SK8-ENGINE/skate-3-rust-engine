"""Regenerate Skyline mod engine loops, pops, and turbo bed (48 kHz mono PCM16)."""
from __future__ import annotations

import json
import math
import random
import struct
import wave
from pathlib import Path

SR = 48000
LOOP_S = 4.0
ANCHORS = (800, 1600, 2800, 4400, 6200, 7800)
RNG = random.Random(0x534B415445)


def firing_hz(rpm: float) -> float:
    return (rpm / 60.0) * 3.0


def write_wav(path: Path, samples: list[float]) -> dict:
    peak = max(abs(s) for s in samples) if samples else 0.0
    if peak > 1e-6:
        scale = min(0.92 / peak, 1.0)
        samples = [s * scale for s in samples]
    rms = math.sqrt(sum(s * s for s in samples) / max(1, len(samples)))
    pcm = b"".join(struct.pack("<h", int(max(-32767, min(32767, s * 32767.0)))) for s in samples)
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(SR)
        wf.writeframes(pcm)
    seam = abs(samples[0] - samples[-1]) if samples else 0.0
    return {
        "file": path.name,
        "frames": len(samples),
        "sample_rate": SR,
        "duration_s": len(samples) / SR,
        "peak": peak,
        "rms": rms,
        "seam_delta": seam,
    }


def synth_engine_loop(ref_rpm: int, load: float) -> list[float]:
    f0 = firing_hz(ref_rpm)
    period = SR / f0
    n = int((LOOP_S * f0) * period)
    if n < SR:
        n = int(LOOP_S * SR)
    coast = 1.0 - load
    out: list[float] = []
    noise = 0.0
    for i in range(n):
        t = i / SR
        phase = t * f0
        body = 0.0
        for h, amp in ((1, 1.0), (2, 0.38), (3, 0.18), (4, 0.10), (6, 0.05)):
            body += amp * math.sin(2.0 * math.pi * h * phase + h * 0.17)
        flutter = 0.03 * math.sin(2.0 * math.pi * phase * 1.004 + 0.4)
        raw = RNG.uniform(-1.0, 1.0)
        noise = noise * 0.965 + raw * 0.035
        hiss = noise * (0.05 + 0.11 * load)
        intake = 0.04 * math.sin(2.0 * math.pi * t * (42.0 + ref_rpm * 0.004))
        s = body * (0.42 + 0.58 * load) + flutter + hiss + intake * coast
        if ref_rpm < 2200:
            s *= 0.82 + 0.18 * math.sin(2.0 * math.pi * t * 2.2)
        out.append(s)
    return out


def synth_turbo() -> list[float]:
    n = int(LOOP_S * SR)
    out: list[float] = []
    noise = 0.0
    for i in range(n):
        t = i / SR
        raw = RNG.uniform(-1.0, 1.0)
        noise = noise * 0.992 + raw * 0.008
        whine = 0.35 * math.sin(2.0 * math.pi * t * 118.0 + 0.2 * math.sin(2.0 * math.pi * t * 3.7))
        out.append((noise * 0.55 + whine) * (0.65 + 0.35 * math.sin(2.0 * math.pi * t * 0.35)))
    return out


def synth_pop(variant: int) -> list[float]:
    duration = 0.055 + variant * 0.012
    n = max(1, int(duration * SR))
    out: list[float] = []
    prev = 0.0
    crack_freq = 2200.0 + variant * 180.0
    for i in range(n):
        t = i / SR
        env = math.exp(-t * 42.0) * (1.0 - math.exp(-t * 280.0))
        raw = RNG.gauss(0.0, 1.0)
        hp = raw - prev * 0.88
        prev = raw
        crack = math.sin(2.0 * math.pi * crack_freq * t) * math.exp(-t * 90.0)
        s = (hp * 0.62 + crack * 0.22) * env
        out.append(s)
    return out


def convert_lift(src: Path, dst: Path) -> dict:
    with wave.open(str(src), "rb") as wf:
        ch = wf.getnchannels()
        sw = wf.getsampwidth()
        rate = wf.getframerate()
        frames = wf.readframes(wf.getnframes())
    if sw != 2:
        raise SystemExit(f"lift.wav must be 16-bit PCM, got width {sw}")
    samples = []
    step = ch * 2
    for i in range(0, len(frames) - step + 1, step):
        mono = 0.0
        for c in range(ch):
            off = i + c * 2
            mono += struct.unpack("<h", frames[off : off + 2])[0] / 32768.0
        samples.append(mono / ch)
    if rate != SR and samples:
        out: list[float] = []
        ratio = rate / SR
        out_len = int(len(samples) / ratio)
        for i in range(out_len):
            pos = i * ratio
            idx = int(pos)
            frac = pos - idx
            a = samples[min(idx, len(samples) - 1)]
            b = samples[min(idx + 1, len(samples) - 1)]
            out.append(a + (b - a) * frac)
        samples = out
    meta = write_wav(dst, samples)
    meta["loop"] = False
    return meta


def main() -> None:
    root = Path(__file__).resolve().parent
    for stale in (900, 2400, 4200, 6600):
        for layer in ("coast", "load"):
            path = root / f"engine_{stale}_{layer}.wav"
            if path.is_file():
                path.unlink()
    sounds: list[dict] = []
    for rpm in ANCHORS:
        for layer, load in (("coast", 0.18), ("load", 0.88)):
            name = f"engine_{rpm}_{layer}.wav"
            meta = write_wav(root / name, synth_engine_loop(rpm, load))
            meta["loop"] = True
            meta["reference_rpm"] = rpm
            meta["layer"] = layer
            sounds.append(meta)
    turbo = write_wav(root / "turbo_loop.wav", synth_turbo())
    turbo["loop"] = True
    sounds.append(turbo)
    for i in range(1, 7):
        pop = write_wav(root / f"pop_{i:02d}.wav", synth_pop(i))
        pop["loop"] = False
        sounds.append(pop)
    lift_src = Path(r"C:\Users\Ethans Desktop 2.0\Downloads\lift.wav")
    if not lift_src.is_file():
        raise SystemExit(f"Missing lift source: {lift_src}")
    sounds.append(convert_lift(lift_src, root / "lift.wav"))
    bank = {
        "provenance": "Engine/turbo/pop synthesis v2; lift from user Downloads/lift.wav.",
        "format": "48000 Hz mono PCM16 WAV",
        "sounds": sounds,
    }
    (root / "soundbank.json").write_text(json.dumps(bank, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {len(sounds)} sounds to {root}")


if __name__ == "__main__":
    main()
