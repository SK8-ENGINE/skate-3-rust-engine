"""List the entries of a .big archive, or extract the ones matching a regex.

usage: py -3.13 tools/world-stream-inspect/big_list.py <big> [regex] [--out DIR]
Without --out prints `<path> <unpacked size>` per entry; with --out writes the matching entries
(decompressed) under DIR, keeping their archive paths.
"""
import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.owned_game.big import BigArchive  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description='List or extract entries of a .big archive.')
    parser.add_argument('big', type=Path)
    parser.add_argument('regex', nargs='?', help='only entries whose path matches (case-insensitive)')
    parser.add_argument('--out', type=Path, help='extract the matching entries here')
    args = parser.parse_args()
    a = BigArchive(args.big)
    pat = re.compile(args.regex, re.I) if args.regex else None
    for e in a.entries:
        if pat is not None and not pat.search(e.path):
            continue
        if args.out:
            p = args.out / BigArchive.safe_relative(e.path)
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(a.read(e))
            print('wrote', p)
        else:
            print(e.path, e.unpacked_size)


if __name__ == '__main__':
    main()
