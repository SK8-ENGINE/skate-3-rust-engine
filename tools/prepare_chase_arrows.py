"""Export retail's chase alert textures (the marker over a ped that warns or chases the player).

Retail's "ChaseArrow" HUD (b191, `825D6EC0` / `825E2B80`) draws one texture per chase state from the
FE-ScreenTextures set: `arrow_warn`, `arrow_chase`, `arrow_tired`, `arrow_giveup` (`data/big/fedynamic.big`).
Writes `<output>/chase_arrows.json` and one RGBA file per state. Output holds game textures and stays private.
"""
from pathlib import Path
import argparse
import hashlib
import json
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from vendor.skate3_ui import project  # noqa: E402
from vendor.skate3_ui.big import BigArchive  # noqa: E402
from vendor.skate3_ui.rw4 import read_textures  # noqa: E402
from vendor.skate3_ui.xenos import decode_rgba, parse_fetch_constant  # noqa: E402

# Chase state (the SendChaseStateMessage stateName, b191) -> retail texture.
STATES = (("warn", "arrow_warn"), ("chase", "arrow_chase"), ("tired", "arrow_tired"), ("giveup", "arrow_giveup"))
VERSION = 1


def prepare(game: Path, output: Path) -> dict:
    big = BigArchive(project.find_big_directory(Path(game)) / project.DYNAMIC_ARCHIVE)
    by_leaf = {e.path.replace("\\", "/").rsplit("/", 1)[-1].lower(): e for e in big.entries}
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    textures = {}
    with tempfile.TemporaryDirectory(prefix="sk8arrow-") as work:
        for state, name in STATES:
            entry = by_leaf.get(name + ".rx2")
            if entry is None:
                raise FileNotFoundError(f"{name}.rx2 not in {project.DYNAMIC_ARCHIVE}")
            raw = Path(work) / (name + ".rx2")
            raw.write_bytes(big.read(entry))
            _, found = read_textures(raw)
            if len(found) != 1:
                raise ValueError(f"{name}.rx2 holds {len(found)} textures, expected 1")
            fetch = parse_fetch_constant(bytes.fromhex(found[0].metadata["fetch_constant_hex"]))
            rgba = decode_rgba(found[0].data, fetch)
            if len(rgba) != fetch.width * fetch.height * 4:
                raise ValueError(f"{name}: decoded {len(rgba)} bytes for {fetch.width}x{fetch.height}")
            file = name + ".rgba"
            (output / file).write_bytes(rgba)
            textures[state] = {"file": file, "width": fetch.width, "height": fetch.height,
                               "sha256": hashlib.sha256(rgba).hexdigest()}
    manifest = {"format": "skate3-chase-arrows", "version": VERSION, "textures": textures}
    (output / "chase_arrows.json").write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")
    print(f"Prepared {len(textures)} chase alert textures: {output}", flush=True)
    return manifest


def runtime_files(root: Path) -> dict:
    """The installed files and their hashes, checked against the manifest."""
    root = Path(root)
    manifest = json.loads((root / "chase_arrows.json").read_text(encoding="utf-8"))
    if manifest.get("format") != "skate3-chase-arrows" or manifest.get("version") != VERSION:
        raise ValueError(f"Unsupported chase alert manifest: {root}")
    files = {"chase_arrows.json": hashlib.sha256((root / "chase_arrows.json").read_bytes()).hexdigest()}
    for state, t in manifest["textures"].items():
        data = (root / t["file"]).read_bytes()
        if len(data) != t["width"] * t["height"] * 4 or hashlib.sha256(data).hexdigest() != t["sha256"]:
            raise ValueError(f"Invalid chase alert texture: {state}")
        files[t["file"]] = t["sha256"]
    return files


def install(assets: Path, source: Path) -> dict:
    """Copy the checked files into `<assets>/private/hud/chase-arrows`; returns {path from assets: sha256}."""
    import shutil
    target = Path(assets) / "private" / "hud" / "chase-arrows"
    target.mkdir(parents=True, exist_ok=True)
    out = {}
    for name, sha in runtime_files(source).items():
        shutil.copyfile(Path(source) / name, target / name)
        if hashlib.sha256((target / name).read_bytes()).hexdigest() != sha:
            raise ValueError(f"Installed chase alert file differs: {name}")
        out[str((target / name).resolve().relative_to(Path(assets).resolve()))] = sha
    return out


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.game, args.output)
