"""Check facts the native AEMS runtime relies on, over every ABKC bank in audiofiles.big:
- the S10A slot order equals the order audio_formats.scan_snr finds the streams in (so the exported
  WAV index = the S10A slot index);
- whether the rebase and interface lists lie below residentsize (on every bank they sit AFTER the
  sample data, at the end of the file, so the runtime needs the whole .abk).
Optionally extracts every .abk/.csi to a folder (input for the other tools here): --extract DIR, which also
writes csi_order.txt (the .csi projects in archive order).

usage: py -3.13 tools/audio-file-inspect/bank_layout_check.py [--extract DIR] [--verbose] [--disc DIR]
"""
import argparse
import os
import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.owned_game.big import BigArchive  # noqa: E402
from tools.asset_pipeline.audio_formats import scan_snr  # noqa: E402



def u32(d, o):
    return struct.unpack_from('>I', d, o)[0]


def main():
    parser = argparse.ArgumentParser(description='Check the ABKC bank layout facts the native AEMS runtime relies on.')
    parser.add_argument('--extract', type=Path, metavar='DIR', help='also extract every .abk/.csi here')
    parser.add_argument('--verbose', action='store_true', help='list every bank whose lists lie after the sample data')
    parser.add_argument('--disc', type=Path, default=Path(os.environ.get('SKATE3_DISC', REPO / '.local/skate3-disc')),
                        help='extracted disc root, the folder holding data/ (default: %(default)s)')
    args = parser.parse_args()
    extract = args.extract
    if extract:
        extract.mkdir(parents=True, exist_ok=True)
    big = BigArchive(args.disc / 'data/audio/audiofiles.big')
    banks = bad_order = bad_resident = 0
    if extract:
        # .csi projects in archive order (the install order; lookup ties go newest first).
        order = [Path(e.path).name for e in big.entries if e.path.lower().endswith('.csi')]
        (extract / 'csi_order.txt').write_text(''.join(name + '\n' for name in order), encoding='utf-8')
    for e in big.entries:
        name = Path(e.path).name
        if not name.lower().endswith(('.abk', '.csi')):
            continue
        d = big.read(e)
        if extract:
            (extract / name).write_bytes(d)
        if d[:4] != b'ABKC':
            continue
        banks += 1
        resident = u32(d, 0x18)
        sfx = u32(d, 0x20)
        assert d[sfx:sfx + 4] == b'S10A', name
        cap = u32(d, sfx + 8)
        slots = []
        for i in range(cap):
            off = u32(d, sfx + 12 + 4 * i)
            if off == 0xFFFFFFFF:
                break
            slots.append(sfx + off)
        scanned = [s.offset for s in scan_snr(d, sfx)]
        if scanned[:len(slots)] != slots or len(scanned) != len(slots):
            bad_order += 1
            print(f'ORDER {name}: slots {len(slots)} scanned {len(scanned)} first diff '
                  f'{next((i for i, (a, b) in enumerate(zip(slots, scanned)) if a != b), None)}')
        rl, il = u32(d, 0x34), u32(d, 0x38)
        n = u32(d, rl)
        ends = [rl + 4 + 4 * n, il + 4 + 12 * u32(d, il)]
        if max(ends) > resident or sfx != resident:
            bad_resident += 1
            if args.verbose:
                print(f'RESIDENT {name}: resident {resident:#x} sfx {sfx:#x} rebase end {ends[0]:#x} iface end {ends[1]:#x}')
    print(f'{banks} banks; slot order mismatches {bad_order}; banks whose rebase/interface lists lie after the '
          f'sample data (so the whole file is needed, not just the resident part): {bad_resident}')


if __name__ == '__main__':
    main()
