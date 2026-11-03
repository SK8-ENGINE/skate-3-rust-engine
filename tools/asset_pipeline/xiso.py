"""Reads named files straight out of an Xbox 360 disc image (XDVDFS), without extracting the whole disc.

A refresh with nothing to rebuild only needs a few disc files (doc 08 "ISO refresh without extracting the disc");
everything else still goes through extract-xiso. The image layout: 2048-byte sectors, the volume descriptor
("MICROSOFT*XBOX*MEDIA") 32 sectors into the game partition, whose start depends on the disc type; each directory is a
binary tree of entries (left and right child offsets in 4-byte units, first sector, size, attributes, name).
"""
import struct
from pathlib import Path

SECTOR = 2048
MAGIC = b'MICROSOFT*XBOX*MEDIA'
# Game partition offsets: plain XISO, XGD2, XGD3, XGD1.
PARTITIONS = (0, 0xFD90000, 0x2080000, 0x18300000)
DIRECTORY = 0x10


class Xiso:
    def __init__(self, path):
        self.path = Path(path)
        with self.path.open('rb') as stream:
            for base in PARTITIONS:
                stream.seek(base + 32 * SECTOR)
                head = stream.read(SECTOR)
                if head[:len(MAGIC)] == MAGIC:
                    break
            else:
                raise ValueError(f'{self.path}: no Xbox disc volume found')
        self.base = base
        self.root = struct.unpack_from('<II', head, len(MAGIC))

    def _entries(self, stream, sector, size):
        stream.seek(self.base + sector * SECTOR)
        data = stream.read(size)
        found, pending, seen = {}, [0], set()
        while pending:
            offset = pending.pop() * 4
            if offset in seen or offset + 14 > len(data):
                continue
            seen.add(offset)
            left, right, start, length, attributes, count = struct.unpack_from('<HHIIBB', data, offset)
            if left == 0xFFFF and right == 0xFFFF:
                continue
            name = data[offset + 14:offset + 14 + count].decode('latin-1')
            found[name.lower()] = (start, length, attributes)
            pending += [child for child in (left, right) if child not in (0, 0xFFFF)]
        return found

    def locate(self, name):
        """(first sector, size) of the file at the disc path `name` (case-insensitive), or None."""
        sector, size = self.root
        parts = [p for p in name.replace('\\', '/').split('/') if p]
        with self.path.open('rb') as stream:
            for index, part in enumerate(parts):
                if size == 0:
                    return None
                entry = self._entries(stream, sector, size).get(part.lower())
                if entry is None:
                    return None
                sector, size, attributes = entry
                if bool(attributes & DIRECTORY) != (index < len(parts) - 1):
                    return None
        return sector, size

    def extract(self, names, destination):
        """Copies each disc path in `names` to destination/<path>; a path the disc lacks is skipped. Returns the paths
        written."""
        written = []
        for name in names:
            where = self.locate(name)
            if where is None:
                continue
            sector, size = where
            target = Path(destination) / name
            target.parent.mkdir(parents=True, exist_ok=True)
            with self.path.open('rb') as source, target.open('wb') as out:
                source.seek(self.base + sector * SECTOR)
                left = size
                while left:
                    chunk = source.read(min(left, 1 << 22))
                    if not chunk:
                        raise ValueError(f'{self.path}: {name} is cut short')
                    out.write(chunk)
                    left -= len(chunk)
            written.append(name)
        return written
