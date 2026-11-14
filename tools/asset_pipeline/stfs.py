"""Read-only reader for Xbox 360 STFS content packages (LIVE / PIRS / CON), the format of Skate 3's DLC.

Only what setup needs: the header (content type, title id, display name), the file table, and copying a file out.
The layout is the public STFS format (Free60 documentation; Xenia's `stfs_container_device` was the reference):
4 KiB blocks after the header, with a level-0 hash table before every 170 data blocks and higher levels above it.
Files stored in consecutive blocks are read directly; a file whose blocks are chained through the hash tables is
refused (none of Skate 3's packages need it, research b374). Nothing here writes into the package.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO

BLOCK = 0x1000
BLOCKS_PER_LEVEL = (0xAA, 0xAA * 0xAA, 0xAA * 0xAA * 0xAA)
MAGICS = (b'LIVE', b'PIRS', b'CON ')
SKATE3_TITLE = 0x454108E6
CONTENT_MARKETPLACE = 0x2


class StfsError(ValueError):
    pass


@dataclass(frozen=True)
class StfsEntry:
    path: str
    size: int
    start_block: int
    block_count: int
    consecutive: bool
    directory: bool


def _u24le(b: bytes, o: int) -> int:
    return b[o] | b[o + 1] << 8 | b[o + 2] << 16


class StfsPackage:
    """One STFS package on disk, opened read-only."""

    def __init__(self, path: Path):
        self.path = Path(path)
        with self.path.open('rb') as f:
            head = f.read(0xAD0E + 0x100)
        if len(head) < 0x412 or head[:4] not in MAGICS:
            raise StfsError(f'{self.path}: not an STFS package')
        self.magic = head[:4].decode('ascii').strip()
        be = lambda o: int.from_bytes(head[o:o + 4], 'big')  # noqa: E731
        self.header_size = be(0x340)
        self.content_type = be(0x344)
        self.title_id = be(0x360)
        self.display_name = head[0x411:0x411 + 0x80].decode('utf-16-be', 'ignore').split('\0', 1)[0]
        vd = head[0x379:0x379 + 0x24]
        if vd[0] != 0x24:
            raise StfsError(f'{self.path}: unsupported volume descriptor (size {vd[0]:#x})')
        self.block_separation = vd[2]
        table_blocks = int.from_bytes(vd[3:5], 'little')
        table_start = _u24le(vd, 5)
        # Xenia: read-only layouts (header rounded to 0xB000) keep one hash table per level, except CON packages.
        if ((self.header_size + 0xFFF) & 0xB000) == 0xB000:
            self.block_shift = 1 if self.magic == 'CON' else 0
        else:
            self.block_shift = 0 if self.block_separation & 1 else 1
        self.data_offset = (self.header_size + 0xFFF) & ~0xFFF
        self.entries = self._file_table(table_start, table_blocks)

    def block_offset(self, block: int) -> int:
        """File offset of data block `block`, past the hash tables interleaved before it."""
        if block >= 0xFFFFFF:
            raise StfsError(f'block {block:#x} out of range')
        physical = block
        for per_level in BLOCKS_PER_LEVEL:
            physical += ((block + per_level) // per_level) << self.block_shift
            if block < per_level:
                break
        return self.data_offset + physical * BLOCK

    def _file_table(self, start: int, count: int) -> list[StfsEntry]:
        raw = bytearray()
        with self.path.open('rb') as f:
            for i in range(count):
                f.seek(self.block_offset(start + i))
                raw += f.read(BLOCK)
        names: list[tuple[str, int]] = []
        entries = []
        for o in range(0, len(raw), 0x40):
            row = bytes(raw[o:o + 0x40])
            flags = row[0x28]
            length = flags & 0x3F
            if length == 0:
                break
            name = row[:length].decode('latin-1')
            parent = int.from_bytes(row[0x32:0x34], 'big', signed=True)
            names.append((name, parent))
            path = name
            seen = 0
            while parent >= 0:
                if parent >= len(names) or seen > 64:
                    raise StfsError(f'{self.path}: bad directory index in the file table')
                path = names[parent][0] + '/' + path
                parent = names[parent][1]
                seen += 1
            entries.append(StfsEntry(
                path=path,
                size=int.from_bytes(row[0x34:0x38], 'big'),
                start_block=_u24le(row, 0x2F),
                block_count=_u24le(row, 0x29),
                consecutive=bool(flags & 0x40),
                directory=bool(flags & 0x80),
            ))
        return entries

    def files(self) -> list[StfsEntry]:
        return [e for e in self.entries if not e.directory]

    def is_skate3_dlc(self) -> bool:
        return self.title_id == SKATE3_TITLE and self.content_type == CONTENT_MARKETPLACE

    def copy(self, entry: StfsEntry, out: BinaryIO) -> None:
        """Write `entry`'s bytes to `out`, a run of consecutive blocks at a time (hash tables skipped)."""
        if entry.directory:
            raise StfsError(f'{entry.path} is a directory')
        if not entry.consecutive and entry.block_count > 1:
            raise StfsError(f'{entry.path}: blocks chained through the hash tables are not supported')
        left = entry.size
        block = entry.start_block
        with self.path.open('rb') as f:
            while left > 0:
                # Data blocks are contiguous on disk until the next level-0 hash table (every 170 blocks).
                run = min(BLOCKS_PER_LEVEL[0] - block % BLOCKS_PER_LEVEL[0], (left + BLOCK - 1) // BLOCK)
                f.seek(self.block_offset(block))
                data = f.read(min(left, run * BLOCK))
                if not data:
                    raise StfsError(f'{entry.path}: package ends early')
                out.write(data)
                left -= len(data)
                block += run

    def read(self, entry: StfsEntry, offset: int, length: int) -> bytes:
        """`length` bytes of `entry` from `offset` (e.g. a BIG's index without copying the archive)."""
        if entry.directory or (not entry.consecutive and entry.block_count > 1):
            raise StfsError(f'{entry.path}: not a consecutive file')
        length = max(0, min(length, entry.size - offset))
        out = bytearray()
        with self.path.open('rb') as f:
            while len(out) < length:
                position = offset + len(out)
                block = entry.start_block + position // BLOCK
                within = position % BLOCK
                run = BLOCKS_PER_LEVEL[0] - block % BLOCKS_PER_LEVEL[0]
                f.seek(self.block_offset(block) + within)
                data = f.read(min(length - len(out), run * BLOCK - within))
                if not data:
                    raise StfsError(f'{entry.path}: package ends early')
                out += data
        return bytes(out)

    def extract(self, entry: StfsEntry, target: Path) -> Path:
        target.parent.mkdir(parents=True, exist_ok=True)
        tmp = target.with_suffix(target.suffix + '.part')
        with tmp.open('wb') as out:
            self.copy(entry, out)
        tmp.replace(target)
        return target


def find_packages(folder: Path) -> list[StfsPackage]:
    """Every Skate 3 marketplace package under `folder` (by header, not by file name); others are skipped."""
    found = []
    for path in sorted(Path(folder).rglob('*')):
        if not path.is_file() or path.stat().st_size < 0xB000:
            continue
        with path.open('rb') as f:
            if f.read(4) not in MAGICS:
                continue
        try:
            package = StfsPackage(path)
        except StfsError:
            continue
        if package.is_skate3_dlc():
            found.append(package)
    return found
