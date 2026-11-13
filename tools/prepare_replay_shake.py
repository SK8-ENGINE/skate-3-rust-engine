"""Export retail's replay camera shake clips (the editor's Shake row: Low / Medium / High).

Retail's shake (research b399 / b400) plays short recorded handheld-camera clips: `data/scene/ICELite.bin` in
`data/big/scene.big`, one ICELite chunk with 14 sub-entries (index 0 "Default" is empty). Each clip's element group
0x21 holds six 16-bit quantised channels at 30 keys per second: SHAKE_QUAT_X/Y/Z and SHAKE_POS_X/Y/Z, each in -1..1
(`82C8FAD8`, dequantised as in `82C8E950`). Writes `<output>/replay_shake.json`. Output holds game data and stays
private (setup only, never committed).
"""
from pathlib import Path
import argparse
import hashlib
import json
import struct
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from owned_game.big import BigArchive  # noqa: E402
from vendor.skate3_ui import project  # noqa: E402

ENTRY = "data/scene/icelite.bin"
VERSION = 1
CLIPS = 14
# The record's element group of the shake channels (A = values - 1 offsets, B = key count) and its six channels.
GROUP_OFFSET = 0x98
CHANNELS = 6


def dequantise(q: int) -> float:
    d = q - 32767
    return d / 32767 if d < 0 else d / 32768


def clips(blob: bytes) -> list:
    """The 14 clips: per clip a list of keys, each [qx, qy, qz, px, py, pz] (empty for a clip without keys)."""
    out = []
    o = 16  # file header + chunk header
    for _ in range(CLIPS):
        _, size, _, _ = struct.unpack_from("<4I", blob, o)
        r = o + 16
        a, b = struct.unpack_from("<HH", blob, r + GROUP_OFFSET)
        keys = []
        if a:
            p = 0x9C + 2 * (a - 2)
            p = (p + 1) & ~1
            p += 2 * (a - 1)
            p = (p + 3) & ~3
            stride = ((16 * b + 31) >> 3) & ~3
            ch = [[dequantise(q) for q in struct.unpack_from(f">{b}H", blob, r + p + k * stride)] for k in range(CHANNELS)]
            keys = [[round(ch[k][i], 6) for k in range(CHANNELS)] for i in range(b)]
        out.append(keys)
        o += 8 + size
    return out


def prepare(game: Path, output: Path) -> dict:
    archive = BigArchive(project.find_big_directory(Path(game)) / "scene.big")
    entry = next((e for e in archive.entries if e.path.lower() == ENTRY), None)
    if entry is None:
        raise FileNotFoundError(f"{ENTRY} not in scene.big")
    manifest = {"format": "skate3-replay-shake", "version": VERSION, "key_rate": 30.0, "clips": clips(archive.read(entry))}
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    (output / "replay_shake.json").write_text(json.dumps(manifest, separators=(",", ":")), encoding="utf-8")
    return manifest


def runtime_files(root: Path) -> dict:
    """The installed file and its hash, checked."""
    path = Path(root) / "replay_shake.json"
    manifest = json.loads(path.read_text(encoding="utf-8"))
    if manifest.get("format") != "skate3-replay-shake" or manifest.get("version") != VERSION or len(manifest.get("clips", [])) != CLIPS:
        raise ValueError(f"Unsupported replay shake file: {path}")
    return {"replay_shake.json": hashlib.sha256(path.read_bytes()).hexdigest()}


def install(assets: Path, source: Path) -> dict:
    """Copy the checked file into `<assets>/private/hud/replay-shake`; returns {path from assets: sha256}."""
    import shutil
    target = Path(assets) / "private" / "hud" / "replay-shake"
    target.mkdir(parents=True, exist_ok=True)
    out = {}
    for name, sha in runtime_files(source).items():
        shutil.copyfile(Path(source) / name, target / name)
        if hashlib.sha256((target / name).read_bytes()).hexdigest() != sha:
            raise ValueError(f"Installed replay shake file differs: {name}")
        out[str((target / name).resolve().relative_to(Path(assets).resolve()))] = sha
    return out


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    m = prepare(args.game, args.output)
    print(f"Replay shake clips: {sum(1 for c in m['clips'] if c)} of {len(m['clips'])} with keys")
