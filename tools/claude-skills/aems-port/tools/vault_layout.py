"""Print a vault class's field layout (field hash, type, offset, count, flags) from the disc schema.
usage: py -3.13 vault_layout.py CLASS [SCHEMA_STEM]
CLASS as 16 hex digits (with or without Hash_). flags bit 2 = a fixed-layout field: the offset is
where retail code reads it in the record (`lwz rX,<offset>(record)`). Reference reading only."""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from tools.asset_pipeline.vlt import vault, unpack, hash64  # noqa: E402

DEFAULT = ROOT / '.local/audio-re/audio-work/audio/database/data/db/skaterschema'


def layout(cls: int, stem: Path = DEFAULT):
    sv, sb, se = vault(stem)
    for _, kind, size, at in se:
        if kind != 0x2A7895AC4A876152:
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


if __name__ == '__main__':
    cls = int(sys.argv[1].replace('Hash_', ''), 16)
    stem = Path(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT
    names = {}
    for n in ('Skate_Collisions', 'Skate_Metal', 'HOM_Set_1', 'EA::Reflection::Int32', 'EA::Reflection::Bool',
              'EA::Reflection::Float', 'Attrib::RefSpec', 'Sk8::Audio::eMaterialNicotineType', 'sk8_foley'):
        names[hash64(n)] = n
    rows = layout(cls, stem)
    if rows is None:
        sys.exit('class not found')
    for fkey, typ, offset, n, maximum, flags, alignment in sorted(rows, key=lambda r: (not r[5] & 2, r[2])):
        print(f'Hash_{fkey:016X} {names.get(typ, hex(typ)):36} off={offset:4} n={n:3} max={maximum} flags={flags} align={alignment}')
