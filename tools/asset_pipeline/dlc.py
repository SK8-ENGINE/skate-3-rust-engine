"""The player's own Skate 3 DLC packages as an optional setup input (research b374 / b410).

Setup never needs DLC: with no folder or no packages everything runs as before. When a folder holds Skate 3
marketplace packages (STFS, title 0x454108E6), their BIG archives are copied out to the conversion work folder and
offered to the exporters next to the disc's archives. Nothing from a package is committed or written back.
"""
from __future__ import annotations

import hashlib
import os
import struct
from pathlib import Path

ENV = 'SKATE3_DLC_DIR'


def folder(source: str | Path | None, saved: str | None = None, game_dir: str | Path | None = None) -> Path | None:
    """The DLC folder: SKATE3_DLC_DIR, else the folder the last setup used, else a `dlc` folder next to the game
    folder (the recomp's layout: `<install>/game/default.xex` + `<install>/dlc/<pack>/454108E6/...`), else a `dlc`
    folder next to this game copy (`game_dir`, the folder with `skate3rust.exe`). The game's start-up check resolves
    the folder by the same rules (`asset_check.rs` `dlc_folder`)."""
    for candidate in (os.environ.get(ENV), saved):
        if candidate and Path(candidate).is_dir():
            return Path(candidate).resolve()
    if source:
        start = Path(source)
        start = start.parent if start.suffix else start
        for root in (start.parent, start):
            if (root / 'dlc').is_dir():
                return (root / 'dlc').resolve()
    if game_dir and (Path(game_dir) / 'dlc').is_dir():
        return (Path(game_dir) / 'dlc').resolve()
    return None


def listing(dlc: Path | None) -> list[str]:
    """Every file in the DLC folder as `relative/path|size`, sorted: recorded in installation.json (`dlc_files`) so the
    game's start-up check sees a pack added or removed and offers the refresh (`asset_check.rs`, same format)."""
    if dlc is None or not dlc.is_dir():
        return []
    return sorted(f'{p.relative_to(dlc).as_posix()}|{p.stat().st_size}' for p in dlc.rglob('*') if p.is_file())


def packages(dlc: Path | None):
    if dlc is None:
        return []
    from .stfs import find_packages
    return find_packages(dlc)


def signature(dlc: Path | None, wanted=None) -> str:
    """A short identity of the packages found (display name, file name, size); '' without any. `wanted(entry_paths)`
    counts only packages with a BIG it accepts (e.g. only those carrying maps for the maps group)."""
    found = packages(dlc)
    if wanted:
        found = [p for p in found if any(wanted(big_paths(p, e)) for e in p.files() if e.path.lower().endswith('.big'))]
    if not found:
        return ''
    digest = hashlib.sha256()
    for package in sorted(found, key=lambda p: p.path.name):
        digest.update(f'{package.display_name}|{package.path.name}|{package.path.stat().st_size}\n'.encode())
    return digest.hexdigest()[:16]


STREAMS = 'data/content/world/stream/'


def stream_districts(paths) -> list[str]:
    """District stream folders in a BIG's entry list that hold a whole map: `<D>_Sim.xst` and Sim cells (b374 / b415;
    the same files a disc `worldDIST_<D>.big` carries)."""
    found: dict[str, set[str]] = {}
    for path in paths:
        if path.lower().startswith(STREAMS):
            parts = path[len(STREAMS):].split('/')
            if len(parts) == 2:
                found.setdefault(parts[0], set()).add(parts[1].lower())
    return sorted(d for d, files in found.items()
                  if f'{d.lower()}_sim.xst' in files and any(f.startswith('csim_') for f in files))


def has_maps(paths) -> bool:
    return bool(stream_districts(paths))


def map_jobs(dlc: Path | None, work: Path) -> list[dict]:
    """One map job per DLC district stream: the BIG (copied out to `work`), its stream folder, the label (the folder
    without its `DIST_` / `Dist_` prefix, so it never collides with a disc map: `MaloofMoneyCupDLC`, `Zen`) and the
    source pack."""
    jobs = []
    for package in packages(dlc):
        for entry in package.files():
            if not entry.path.lower().endswith('.big'):
                continue
            districts = stream_districts(big_paths(package, entry))
            if not districts:
                continue
            target = work / package.path.name / entry.path
            if not (target.is_file() and target.stat().st_size == entry.size):
                package.extract(entry, target)
            for district in districts:
                label = district[5:] if district.lower().startswith('dist_') else district
                jobs.append({'archive': target, 'district': district, 'label': label,
                             'source': 'dlc:' + package.display_name})
    return jobs


def big_paths(package, entry) -> list[str]:
    """The entry paths of a BIG inside a package, from its index alone (the archive is not copied)."""
    from tools.owned_game.big import BigArchive
    head = package.read(entry, 0, 48)
    metadata = int.from_bytes(head[12:16], 'big') + int.from_bytes(head[16:20], 'big')
    # BigArchive's index parser needs only the index bytes and the archive's size (for its bounds checks).
    index = BigArchive.__new__(BigArchive)
    index.path = Path(entry.path)
    index.file_size = entry.size
    index.metadata = package.read(entry, 0, metadata)
    return [e.path.replace('\\', '/') for e in index._parse()]


def unlocks(dlc: Path | None) -> list[dict]:
    """The `KIND,name` lines of every package's `data/unlocks/*.unlock` text files (research b458: retail rebuilds its
    second unlock list from them, `82505220`, and the Challenge Map's location test asks it), as
    {source, kind, name}; read from the package without copying its BIGs. Kinds stay as written (LOCATIONS, ...)."""
    from tools.owned_game.big import BigArchive
    result = []
    for package in packages(dlc):
        for entry in package.files():
            if not entry.path.lower().endswith('.big'):
                continue
            head = package.read(entry, 0, 48)
            index = BigArchive.__new__(BigArchive)
            index.path = Path(entry.path)
            index.file_size = entry.size
            index.metadata = package.read(entry, 0, int.from_bytes(head[12:16], 'big') + int.from_bytes(head[16:20], 'big'))
            for item in index._parse():
                path = item.path.replace('\\', '/').lower()
                if not (path.startswith('data/unlocks/') and path.endswith('.unlock')):
                    continue
                text = BigArchive.decode(item, package.read(entry, item.offset, item.stored_size)).decode('latin-1')
                for line in text.splitlines():
                    kind, _, name = line.strip().partition(',')
                    if kind and name.strip():
                        result.append({'source': package.display_name, 'file': path.rsplit('/', 1)[-1],
                                       'kind': kind.strip().upper(), 'name': name.strip()})
    return result


def extract_bigs(dlc: Path | None, work: Path, wanted=None) -> list[Path]:
    """Copies each package's BIG archives to `work/<package file>/<big>` (kept while the size matches) and returns
    their paths. `wanted(entry_paths)` sees the BIG's entry list (read from its index) and limits which are copied."""
    result = []
    for package in packages(dlc):
        for entry in package.files():
            if not entry.path.lower().endswith('.big') or (wanted and not wanted(big_paths(package, entry))):
                continue
            target = work / package.path.name / entry.path
            if not (target.is_file() and target.stat().st_size == entry.size):
                package.extract(entry, target)
            result.append(target)
    return result


def vaults(bigs, schema_stem: Path, names, work: Path) -> list[dict]:
    """The DLC packs' collection vaults (the `.vlt` / `.bin` pairs directly in `data/db/`; challenge vaults in its
    subfolders are not collections) converted with the disc's schema (research b412: the rows only add to the disc's
    and inherit from them), each row tagged with its vault so its text offsets resolve against its own `.bin`; plus the
    packs' global locators keyed like `map_starts.locators` (folder, locator) -> matrices."""
    from tools.owned_game.big import BigArchive
    from .teleports import location_records
    from .vlt import convert, vault
    result = []
    for big in bigs:
        archive = BigArchive(big)
        entries = {e.path.replace('\\', '/').lower(): e for e in archive.entries}
        located: dict = {}
        own: dict = {}
        for path, entry in entries.items():
            if path.startswith('data/content/global_locators/') and path.endswith('.rx2'):
                folder = path.rsplit('/', 2)[-2]
                for record in location_records(archive.read(entry)):
                    located.setdefault((folder.casefold(), record['locator']), []).append(record['matrix'])
            elif path.startswith(STREAMS) and path.endswith('_sim.loc') and path.count('/') == 5:
                # A stream's own LocationTable (b418: the only home of some park starts, e.g. Fn_Start_Zen).
                folder = path.rsplit('/', 2)[-2]
                for locator, matrix in sim_locations(archive.read(entry)):
                    own.setdefault((folder.casefold(), locator), []).append(matrix)
        stems = sorted(p[:-4] for p in entries if p.startswith('data/db/') and p.count('/') == 2 and p.endswith('.vlt')
                       and p[:-4] + '.bin' in entries)
        for stem in stems:
            name = stem.rsplit('/', 1)[-1]
            target = work / big.stem / name
            target.parent.mkdir(parents=True, exist_ok=True)
            for suffix in ('.vlt', '.bin'):
                (target.parent / (name + suffix)).write_bytes(archive.read(entries[stem + suffix]))
            converted = convert(schema_stem, target, names)
            for row in converted['collections']:
                row['vault'] = name
            _, binary, _ = vault(target)
            result.append({'vault': name, 'big': big, 'converted': converted, 'binary': binary, 'locators': located,
                           'stream_locators': own})
            located, own = {}, {}
    return result


def load(dlc: Path | None, game_root: Path, work: Path) -> tuple[list[dict], list[dict]]:
    """The map jobs and collection vaults of the player's map packs (their BIGs copied to `work/dlc`; the disc's
    schema from db.big). ([], []) without map packs."""
    jobs = map_jobs(dlc, work / 'dlc')
    if not jobs:
        return [], []
    from tools.owned_game.big import BigArchive
    schema = work / 'dlc-schema'
    if not (schema / 'data/db/skaterschema.vlt').is_file():
        database = BigArchive(Path(game_root) / 'data/big/db.big')
        database.extract_entries([e for e in database.entries
                                  if Path(e.path).name.lower() in {'skaterschema.bin', 'skaterschema.vlt'}], schema)
    names = (Path(__file__).resolve().parent / 'names.txt').read_text(encoding='utf-8').splitlines()
    return jobs, vaults(sorted({j['archive'] for j in jobs}), schema / 'data/db/skaterschema', names, work / 'dlc-vaults')


def sim_locations(data: bytes) -> list[tuple[str, list[list[float]]]]:
    """`<D>_Sim.loc` records: (name, 4x4 rows) from the XML LocationTable (row-major, the last row the position: the
    same layout as the global locators' matrices)."""
    import xml.etree.ElementTree as ET
    out = []
    for location in ET.fromstring(data.decode('utf-8', 'replace')).iter('Location'):
        name, transform = location.findtext('Name'), location.findtext('Transform')
        values = [float(v) for v in (transform or '').split(',') if v.strip()]
        if name and len(values) == 16:
            out.append((name.strip(), [values[i:i + 4] for i in range(0, 16, 4)]))
    return out


def merged(base: dict, dlc_vaults: list[dict]) -> dict:
    """The disc's converted collections with the DLC rows after them (a DLC row never replaces a disc row)."""
    seen = {(r['class'].casefold(), r['key'].casefold()) for r in base['collections']}
    rows = list(base['collections'])
    for v in dlc_vaults:
        rows += [r for r in v['converted']['collections'] if (r['class'].casefold(), r['key'].casefold()) not in seen]
    return {**base, 'collections': rows}


def _strings(data: bytes) -> list[str]:
    # Same layout as vendor/skate3_ui/language.py: count at 8, header 28, pool descriptor (8 bytes) at the offset at
    # 16, then NUL-separated strings. The DLC label tables end with one NUL where the disc's end with two, so the
    # last (empty) string is padded here instead of rejecting the table (and extra empty padding is dropped).
    count, header, pool = struct.unpack_from('<III', data, 8)
    if header != 28 or pool + 8 > len(data):
        raise ValueError('unsupported language table')
    strings = [s.decode('latin-1') for s in data[pool + 8:].split(b'\0')]
    while len(strings) < count + 2:
        strings.append('')
    # Some DLC tables carry one more trailing NUL instead: drop empty padding past the declared count.
    while len(strings) > count + 2 and strings[-1] == '':
        strings.pop()
    if len(strings) != count + 2:
        raise ValueError(f'expected {count + 2} language strings, found {len(strings)}')
    return strings


def language(archive, language_name: str = 'english') -> dict[str, str]:
    """Label -> text from every labels / <language> table pair in a DLC BIG (e.g. the creator pack's
    LANGUAGE_Labels_creator_* + LANGUAGE_English_creator_*); HISTOGRAM tables are skipped."""
    tables = {}
    for entry in archive.entries:
        path = entry.path.replace('\\', '/')
        lower = path.lower()
        if not lower.startswith('data/fe/languages/') or 'histogram' in lower or not lower.endswith('.bin'):
            continue
        folder_name, name = lower.split('/')[-2:]
        if folder_name in ('labels', language_name):
            suffix = name.split('_', 2)[-1]
            tables.setdefault(suffix, {})[folder_name] = entry
    result = {}
    for pair in tables.values():
        if 'labels' in pair and language_name in pair:
            labels = _strings(archive.read(pair['labels']))
            values = _strings(archive.read(pair[language_name]))
            if len(labels) == len(values):
                for label, value in zip(labels, values):
                    if label:
                        result.setdefault(label.strip(), value)
    return result
