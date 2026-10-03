"""Resident memory of the world emitter banks, per .ems emitter file of the install (or per group of
files): the banks its played records use (kind 1, flags 0, bank in the native install) and their size when
decoded the way the native runtime keeps them (16-bit WAV -> planar f32 = 2x the data chunk) plus the
.abk bytes it keeps.

usage: py -3.13 tools/audio-bench/emitter_bank_memory.py [--assets DIR] [--group NAME=ems1,ems2 ...]
  --assets  the asset root (default $SKATE_ASSETS or assets, the dev junction)
  --group   sum several .ems files under one name (e.g. a map's sfx_/music_/reverb_ files); repeatable.
            Without --group, one row per .ems file in the manifest.
"""
import argparse
import json
import os
import struct
import sys

_cache = {}
root = None
m = None


def load(assets):
    """Read the install's audio manifest (call before bank_size / records)."""
    global root, m
    root = os.path.join(assets, 'private', 'audio')
    with open(os.path.join(root, 'audio_manifest.json')) as f:
        m = json.load(f)
    _cache.clear()


def data_bytes(path):
    with open(path, 'rb') as f:
        b = f.read()
    at = 12
    while at + 8 <= len(b):
        size = struct.unpack_from('<I', b, at + 4)[0]
        if b[at:at + 4] == b'data':
            return min(size, len(b) - at - 8)
        at += 8 + size + (size & 1)
    return 0


def bank_size(stem):
    if stem not in _cache:
        pcm = sum(2 * data_bytes(os.path.join(root, e['file'])) for e in m['banks'].get(stem, [])
                  if os.path.exists(os.path.join(root, e['file'])))
        _cache[stem] = pcm + os.path.getsize(os.path.join(root, m['aems']['banks'][stem]))
    return _cache[stem]


def records(ems_files):
    """The played native emitter records of these .ems files (kind 1, flags 0, bank in the install)."""
    return [r for f in ems_files for r in m['emitters'].get(f, [])
            if r['kind'] == 1 and r['flags'] == 0 and r.get('bank') and r['bank'] in m['aems']['banks']]


def table(groups):
    for name, files in groups.items():
        banks = list(dict.fromkeys(r['bank'] for r in records(files)))
        if not banks:
            continue
        total = sum(bank_size(b) for b in banks)
        big = sorted(((bank_size(b), b) for b in banks), reverse=True)[:3]
        print(f'{name:24s} {len(banks):3d} banks {total / 2**20:7.1f} MiB   largest: '
              + ', '.join(f'{b} {s / 2**20:.1f}' for s, b in big))


def parse_groups(values):
    groups = {}
    for v in values:
        name, _, files = v.partition('=')
        groups[name] = [f for f in files.split(',') if f]
    return groups


def main():
    parser = argparse.ArgumentParser(description='Resident memory of the world emitter banks per .ems file or group.')
    parser.add_argument('--assets', default=os.environ.get('SKATE_ASSETS', 'assets'), help='asset root (default: %(default)s)')
    parser.add_argument('--group', action='append', default=[], metavar='NAME=ems1,ems2', help='repeatable')
    args = parser.parse_args()
    load(args.assets)
    groups = parse_groups(args.group) or {f: [f] for f in sorted(m.get('emitters', {}))}
    table(groups)


if __name__ == '__main__':
    sys.exit(main())
