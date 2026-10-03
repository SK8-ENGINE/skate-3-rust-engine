"""Dump the world emitter files (.ems) in audiofiles.big: per file the record count and the most used
sounds, and with --records every record (index, flags, position, extent, scalars, sound, gains).

Sound ids are 64-bit name ids (tools/asset_pipeline/audio_formats.name_id); they are resolved against
the stems of every member of the same archive (as-is and lower case), unresolved ids print as ?<hex>.

usage: py -3.13 tools/audio-file-inspect/ems_dump.py [FILTER ...] [--records] [--disc DIR]
  FILTER: substrings of the .ems path (e.g. sfx_university); default every .ems file.
"""
import argparse
import os
import sys
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.owned_game.big import BigArchive  # noqa: E402
from tools.asset_pipeline.audio_formats import ems_emitters, name_id  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description='Dump the .ems world emitter files of audiofiles.big.')
    parser.add_argument('filters', nargs='*', help='substrings of the .ems path (default: all)')
    parser.add_argument('--records', action='store_true', help='print every record')
    parser.add_argument('--disc', type=Path, default=Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')),
                        help='extracted disc root, the folder holding data/ (default: %(default)s)')
    args = parser.parse_args()
    big = BigArchive(args.disc / 'data/audio/audiofiles.big')
    lookup = {}
    for e in big.entries:
        stem = Path(e.path).stem
        for v in (stem, stem.lower()):
            try:
                lookup[name_id(v)] = stem
            except UnicodeEncodeError:
                pass
    total = resolved = 0
    for e in big.entries:
        if not e.path.lower().endswith('.ems'):
            continue
        if args.filters and not any(f.lower() in e.path.lower() for f in args.filters):
            continue
        records = ems_emitters(big.read(e))
        names = Counter()
        for r in records:
            name = lookup.get(r['sound_id'], '?%016X' % r['sound_id'])
            names[name] += 1
            total += 1
            resolved += not name.startswith('?')
        print(f'{e.path}: {len(records)} records; top: {names.most_common(10)}')
        if args.records:
            for r in records:
                name = lookup.get(r['sound_id'], '?%016X' % r['sound_id'])
                print(f"  #{r['index']:<4} flags {r['flags']:<3} {name:32} pos {[round(v, 2) for v in r['position']]} "
                      f"ext {[round(v, 2) for v in r['extent']]} scalars {[round(v, 3) for v in r['scalars']]} "
                      f"gains {[round(v, 3) for v in r['gains']]}")
    print(f'records {total}, sound ids resolved {resolved}')


if __name__ == '__main__':
    main()
