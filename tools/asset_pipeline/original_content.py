"""Lossless, indexed extraction of the selected owned game tree.

Keep each bank in its own namespace: duplicated names in different banks must
not overwrite one another. Container decoding is separate from runtime format
conversion; unported audio/frontend/etc. payloads remain original private data.
"""
from pathlib import Path
import hashlib
import json
import shutil

from tools.owned_game.big import BigArchive


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def destination(root, relative):
    target = (root / BigArchive.safe_relative(relative)).resolve()
    if not target.is_relative_to(root.resolve()):
        raise ValueError(f"Original content path escaped output: {relative}")
    return target


def extract(game_root, output, report=print):
    game_root, output = game_root.resolve(strict=True), output.resolve()
    if not game_root.is_dir():
        raise ValueError("Owned game source must be a directory")
    if output.is_relative_to(game_root) or game_root.is_relative_to(output):
        raise ValueError("Original content output must be separate from the owned source")
    output.mkdir(parents=True, exist_ok=True)
    index = output / "manifest.json"
    prior = json.loads(index.read_text(encoding="utf-8")) if index.is_file() else {}
    records = []
    old = {r["output"]: r for r in prior.get("files", [])}
    claimed = set()

    def claim(relative):
        key = relative.casefold()
        if key in claimed:
            raise ValueError(f"Ambiguous original content path: {relative}")
        claimed.add(key)
        return destination(output, relative)

    def bank(path, relative, lineage, depth=0):
        if depth > 16:
            raise ValueError("Excessive nested original archives")
        archive = BigArchive(path)
        identity = digest(path)
        report(f"Extracting original bank: {lineage} ({len(archive.entries)} entries)")
        for entry in archive.entries:
            name = BigArchive.safe_relative(entry.path).as_posix()
            member = relative + "/" + name
            target = claim(member)
            previous = old.get(member, {})
            if not (previous.get("archive_sha256") == identity and target.is_file()
                    and target.stat().st_size == entry.unpacked_size
                    and digest(target) == previous.get("sha256")):
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(entry))
            records.append(dict(output=member, archive=lineage, entry=entry.path,
                                archive_sha256=identity, size=entry.unpacked_size, sha256=digest(target)))
            if name.lower().endswith(".big"):
                bank(target, member + ".contents", lineage + "!/" + name, depth + 1)

    for source in sorted(game_root.rglob("*")):
        if not source.is_file():
            continue
        relative = source.relative_to(game_root).as_posix()
        if source.suffix.lower() == ".big":
            bank(source, "banks/" + relative, relative)
        else:
            name = "loose/" + relative
            target = claim(name)
            sha256 = digest(source)
            if not target.is_file() or digest(target) != sha256:
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
            if digest(target) != sha256:
                raise ValueError(f"Original file copy failed: {relative}")
            records.append(dict(output=name, source=relative, size=source.stat().st_size, sha256=sha256))
    manifest = dict(version=1, files=records)
    temporary = index.with_suffix(".json.new")
    temporary.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    temporary.replace(index)
    return manifest


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    extract(args.game, args.output)
