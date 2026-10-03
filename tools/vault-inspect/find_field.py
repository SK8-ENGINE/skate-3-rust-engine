"""Find vault field hashes in the install's converted database (skater-collections.json): prints class,
collection, field, type and the decoded value (big-endian float or int for 4-byte values) of every hit.

usage: py -3.13 tools/vault-inspect/find_field.py HASH [HASH ...] [--vault JSON]
  HASH: 16 hex digits, with or without Hash_.
  --vault defaults to assets/private/stock/skater-collections.json (written by setup).
"""
import argparse
import json
import struct
from pathlib import Path

DEFAULT = Path(__file__).resolve().parents[2] / 'assets/private/stock/skater-collections.json'


def main():
    parser = argparse.ArgumentParser(description='Find vault field hashes in skater-collections.json.')
    parser.add_argument('hashes', nargs='+', help='16 hex digits, with or without Hash_')
    parser.add_argument('--vault', type=Path, default=DEFAULT, help='converted database (default: %(default)s)')
    args = parser.parse_args()
    want = {('Hash_' + h.replace('Hash_', '').upper().zfill(16)) for h in args.hashes}
    for c in json.loads(args.vault.read_text())['collections']:
        for w in want & set(c['fields']):
            v = c['fields'][w]
            h = ''.join(v.get('data', '').split())
            val = h
            if 'Float' in v['type'] and len(h) == 8:
                val = struct.unpack('>f', bytes.fromhex(h))[0]
            elif len(h) == 8:
                val = struct.unpack('>i', bytes.fromhex(h))[0]
            print(c['class'], c['key'], w, v['type'], val)


if __name__ == '__main__':
    main()
