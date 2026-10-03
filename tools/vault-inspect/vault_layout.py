"""Print a vault class's field layout (field hash, type, offset, count, flags) from the disc schema.

usage: py -3.13 tools/vault-inspect/vault_layout.py CLASS --schema STEM [--name NAME ...]
  CLASS: 16 hex digits (with or without Hash_).
  --schema: path of skaterschema without extension (the folder must hold skaterschema.vlt and .bin;
            extract them from data/big/db.big with tools/world-stream-inspect/big_list.py db.big skaterschema --out DIR).
  --name: type names to label (their 64-bit hash is shown by name when it matches); repeatable.
flags bit 2 = a fixed-layout field: the offset is where the record keeps it.
"""
import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.asset_pipeline.vlt import vault, unpack, hash64  # noqa: E402

CLASS_EXPORT = 0x2A7895AC4A876152
# Generic reflection type names, so the common field types print by name.
TYPE_NAMES = ('EA::Reflection::Int32', 'EA::Reflection::Bool', 'EA::Reflection::Float', 'Attrib::RefSpec')


def layout(cls: int, stem: Path):
    sv, sb, se = vault(stem)
    for _, kind, size, at in se:
        if kind != CLASS_EXPORT:
            continue
        key, reserve, count, defs, static_size, static, lay = unpack('QIIIIII', sv, at)
        if key != cls:
            continue
        rows = []
        for i in range(count):
            fkey, typ, offset, n, maximum, flags, alignment = unpack('QQHHHBB', sb, defs + i * 24)
            rows.append((fkey, typ, offset, n, maximum, flags, alignment))
        return rows
    return None


def main():
    parser = argparse.ArgumentParser(description="Print a vault class's field layout from the disc schema.")
    parser.add_argument('cls', metavar='CLASS', help='16 hex digits, with or without Hash_')
    parser.add_argument('--schema', type=Path, required=True, help='skaterschema path without extension')
    parser.add_argument('--name', action='append', default=[], help='extra type name to label; repeatable')
    args = parser.parse_args()
    cls = int(args.cls.replace('Hash_', ''), 16)
    names = {hash64(n): n for n in (*TYPE_NAMES, *args.name)}
    rows = layout(cls, args.schema)
    if rows is None:
        sys.exit('class not found')
    for fkey, typ, offset, n, maximum, flags, alignment in sorted(rows, key=lambda r: (not r[5] & 2, r[2])):
        print(f'Hash_{fkey:016X} {names.get(typ, hex(typ)):36} off={offset:4} n={n:3} max={maximum} flags={flags} align={alignment}')


if __name__ == '__main__':
    main()
