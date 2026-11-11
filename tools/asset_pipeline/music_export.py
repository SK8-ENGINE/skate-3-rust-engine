"""The in-game music (docs/hails-additions/11-audio.md#music-banks, research b1015): the disc's three EA PathFinder
sets (data/audio/music/{ipod,world,game}.mpf with their .mus streams) decoded once with vgmstream (each segment is a
subsong) and packed into one FLAC music bank per set by the game executable (`--pack-music`). The WAVs are temporary:
each set's are deleted once its bank is written. The music is the player's own disc content, exported locally."""
import os
import re
import shutil
import subprocess
from pathlib import Path

SETS = ('ipod', 'world', 'game')
# Subsongs per vgmstream process (the decode workers each take ranges of this size).
_RANGE = 200


def music_requested() -> bool:
    """On by default; `SKATE_SETUP_MUSIC=0` skips the ~2.7 GB of music banks."""
    return os.environ.get('SKATE_SETUP_MUSIC', '') != '0'


def subsong_count(vgmstream: Path, mpf: Path) -> int:
    from . import install as engine
    with engine.spawn([str(vgmstream), '-m', str(mpf)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
        text = process.stdout.read()
        process.wait()
    match = re.search(r'stream count:\s*(\d+)', text)
    return int(match.group(1)) if match else 1


def _decode_range(vgmstream: Path, mpf: Path, folder: Path, first: int, last: int, log) -> None:
    from . import install as engine
    with engine.spawn([str(vgmstream), '-s', str(first), '-S', str(last), '-o', '?s.wav', str(mpf)], cwd=folder,
                      stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
        output = process.stdout.read()
        if process.wait():
            log.write(output)
            raise RuntimeError(f'vgmstream failed on {mpf.name} subsongs {first}..{last}')


def pack(game_exe: Path, folder: Path, count: int, bank: Path, log) -> str:
    from . import install as engine
    with engine.spawn([str(game_exe), '--pack-music', str(folder), str(count), str(bank)],
                      stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
        text = process.stdout.read()
        code = process.wait()
    log.write(text)
    log.flush()
    if code or 'MUSIC_BANK_READY' not in text:
        bank.unlink(missing_ok=True)
        raise RuntimeError('Music pack failed: ' + (text.strip().splitlines() or ['no output'])[-1])
    return text.strip().splitlines()[-1]


def export(game_exe, vgmstream, music_root, output, work, log, report=print) -> dict:
    """Writes output/music/<set>.bank for each set on the disc; returns the manifest entry
    {set: {'bank': 'music/<set>.bank', 'segments': n}}."""
    from .audio_export import _in_order
    music_root, output, work = Path(music_root), Path(output), Path(work)
    (output / 'music').mkdir(parents=True, exist_ok=True)
    entry = {}
    for name in SETS:
        mpf = music_root / f'{name}.mpf'
        if not mpf.is_file():
            continue
        count = subsong_count(vgmstream, mpf)
        folder = work / name
        shutil.rmtree(folder, ignore_errors=True)
        folder.mkdir(parents=True)
        report(f'Decoding music: {name} ({count} segments)')
        ranges = [(a, min(a + _RANGE - 1, count)) for a in range(1, count + 1, _RANGE)]
        _in_order(lambda r: _decode_range(vgmstream, mpf, folder, r[0], r[1], log), ranges)
        report(f'Packing music: {name}')
        bank = output / 'music' / f'{name}.bank'
        log.write(pack(game_exe, folder, count, bank, log) + '\n')
        shutil.rmtree(folder, ignore_errors=True)
        # The PathFinder graph (which segments make a track, how the music branches) next to its bank; the engine
        # parses it (skate_audio::music::pathfinder::format).
        shutil.copyfile(mpf, output / 'music' / f'{name}.mpf')
        entry[name] = {'bank': f'music/{name}.bank', 'graph': f'music/{name}.mpf', 'segments': count}
    return entry
