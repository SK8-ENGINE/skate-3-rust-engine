"""Make the music SDK test mod's bank and playlist from self-made tones (no game or licensed music).

    py -3.13 make_bank.py <path to skate3rust.exe> [--from <music folder> [--count N]]

Writes three 8 s tone tracks to music/src (named "Dev Tones - <title>.wav", the packer's artist - title
fallback), packs them with `skate3rust --pack-music-folder` into music/dev-tones.bank, writes
music/playlist.json with `--music-playlist`, and adds disc soundtrack track 1 at its end (a mixed playlist).
With --from, the first N (default 3) music files of that folder (name order) go into the bank instead, for a
listening run with the player's own music: that bank stays local (ignored by git) and is never shared.
The bank and the WAVs are build output (ignored by git); run this again after a clean checkout.
"""
from __future__ import annotations

import json
import math
import shutil
import struct
import subprocess
import sys
import wave
from pathlib import Path

RATE = 44100
HERE = Path(__file__).resolve().parent
TRACKS = [("Low Hum", 110.0), ("Mid Pulse", 330.0), ("High Shimmer", 880.0)]


def tone(path: Path, freq: float, seconds: float = 8.0) -> None:
    frames = bytearray()
    n = int(RATE * seconds)
    for i in range(n):
        t = i / RATE
        env = min(1.0, t / 0.2, (seconds - t) / 0.5)
        pulse = 0.6 + 0.4 * math.sin(2 * math.pi * 1.5 * t)
        v = 0.25 * env * pulse * (math.sin(2 * math.pi * freq * t) + 0.3 * math.sin(4 * math.pi * freq * t))
        s = int(max(-1.0, min(1.0, v)) * 32767)
        frames += struct.pack('<hh', s, s)
    with wave.open(str(path), 'wb') as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(bytes(frames))


def main() -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    args = sys.argv[1:]
    if not args or args[0].startswith('--'):
        print(__doc__)
        return 2
    exe = args[0]
    source = Path(args[args.index('--from') + 1]) if '--from' in args else None
    count = int(args[args.index('--count') + 1]) if '--count' in args else 3
    music = HERE / 'music'
    src = music / 'src'
    if src.exists():
        shutil.rmtree(src)
    src.mkdir(parents=True)
    if source is None:
        for title, freq in TRACKS:
            tone(src / f'Dev Tones - {title}.wav', freq)
    else:
        files = sorted(f for f in source.iterdir() if f.is_file() and f.suffix.lower() in {'.mp3', '.flac', '.ogg', '.wav', '.m4a'})
        for f in files[:count]:
            shutil.copy2(f, src / f.name)
    bank = music / 'dev-tones.bank'
    playlist = music / 'playlist.json'
    subprocess.run([exe, '--pack-music-folder', str(src), str(bank)], check=True)
    subprocess.run([exe, '--music-playlist', str(playlist), 'Music SDK test', str(bank)], check=True)
    data = json.loads(playlist.read_text(encoding='utf-8'))
    data['tracks'].append({'soundtrack': 1})
    playlist.write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')
    shutil.rmtree(src)
    titles = [t.get('name', '') for t in data['tracks'] if 'bank' in t]
    print(f'{bank} and {playlist} written ({len(data["tracks"])} entries)')
    print('first_title=' + (titles[0] if titles else ''))
    print('second_title=' + (titles[1] if len(titles) > 1 else ''))
    return 0


if __name__ == '__main__':
    sys.exit(main())
