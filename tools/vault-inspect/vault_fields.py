"""Print vault fields from the install's converted database (skater-collections.json) by class /
collection / field keys, with the collection's parent. Floats and ints are decoded; arrays list their items.

usage: py -3.13 tools/vault-inspect/vault_fields.py CLASS [COLLECTION] [FIELD ...] [--vault JSON]
  CLASS / FIELD: 16 hex digits (with or without Hash_); COLLECTION: a name ('default') or hex.
  --vault defaults to assets/private/stock/skater-collections.json (written by setup).
"""
import argparse
import json
import struct
from pathlib import Path

DEFAULT = Path(__file__).resolve().parents[2] / 'assets/private/stock/skater-collections.json'


def norm(k):
    k = k.strip()
    if k.lower() == 'default':
        return 'default'
    return k if k.startswith('Hash_') else 'Hash_' + k.upper().replace('0X', '').zfill(16)


def decode(v):
    t = v.get('type', '')

    def one(h):
        b = bytes.fromhex(h)
        if 'Float' in t and len(b) == 4:
            return round(struct.unpack('>f', b)[0], 6)
        if len(b) == 4:
            return struct.unpack('>i', b)[0]
        if len(b) == 1:
            return b[0]
        return h
    if 'array' in v:
        return t, [one(x) for x in v['array']['items']]
    return t, one(v['data'])


def main():
    parser = argparse.ArgumentParser(description='Print vault fields by class / collection / field.')
    parser.add_argument('cls', metavar='CLASS')
    parser.add_argument('collection', nargs='?', metavar='COLLECTION')
    parser.add_argument('fields', nargs='*', metavar='FIELD')
    parser.add_argument('--vault', type=Path, default=DEFAULT, help='converted database (default: %(default)s)')
    args = parser.parse_args()
    d = json.loads(args.vault.read_text())
    items = d['collections'] if 'collections' in d else d
    items = list(items.values()) if isinstance(items, dict) else items
    cls = norm(args.cls)
    col = norm(args.collection) if args.collection else None
    fields = [norm(f) for f in args.fields]
    by = {c['key']: c for c in items if c['class'] == cls}
    for key, c in by.items():
        if col and key != col:
            continue
        print(f'== {cls} {key} parent={c.get("parent")}')
        for f, v in sorted(c['fields'].items()):
            if fields and f not in fields:
                continue
            print(f'  {f} {decode(v)}')


if __name__ == '__main__':
    main()
