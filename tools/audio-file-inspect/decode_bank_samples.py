"""Decode a disc bank's samples to WAVs the way setup does (S10A slot i = <out>/<stem>/<i:04d>.wav),
for banks the install does not export (e.g. PatchBank_SpiderCracks). Headless, with vgmstream-cli.

usage: py -3.13 tools/audio-file-inspect/decode_bank_samples.py STEM [STEM...] [--banks DIR] [--out DIR] [--vgmstream EXE]
  --banks      extracted disc banks (default .local/audio-file-inspect/banks, from bank_layout_check.py --extract)
  --out        output root (default .local/audio-file-inspect/bank-wavs)
  --vgmstream  vgmstream-cli (default: the one setup downloads, data/tools/vgmstream-cli/vgmstream-cli.exe)
Reference data for local tests only; never commit the output."""
import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))
from tools.asset_pipeline.audio_export import _streams  # noqa: E402
from tools.asset_pipeline.audio_formats import standalone  # noqa: E402

VGMSTREAM = REPO / 'data/tools/vgmstream-cli/vgmstream-cli.exe'


def main():
    parser = argparse.ArgumentParser(description="Decode a disc bank's samples to WAVs the way setup does.")
    parser.add_argument('stems', nargs='+', help='bank stems, e.g. PatchBank_SpiderCracks')
    parser.add_argument('--banks', type=Path, default=REPO / '.local/audio-file-inspect/banks')
    parser.add_argument('--out', type=Path, default=REPO / '.local/audio-file-inspect/bank-wavs')
    parser.add_argument('--vgmstream', type=Path, default=VGMSTREAM)
    args = parser.parse_args()
    banks, out = args.banks, args.out
    for stem in args.stems:
        data = (banks / f'{stem}.abk').read_bytes()
        streams = _streams(stem, data)
        target = out / stem
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            for i, stream in enumerate(streams):
                snr = work / f'{i:04d}.snr'
                snr.write_bytes(standalone(data, stream))
                wav = target / f'{i:04d}.wav'
                done = subprocess.run([str(args.vgmstream), '-i', '-o', str(wav), str(snr)], capture_output=True,
                                      creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
                if done.returncode or not wav.is_file():
                    sys.exit(f'{stem} #{i}: vgmstream failed: {done.stderr.decode(errors="replace")[-400:]}')
        print(f'{stem}: {len(streams)} samples -> {target}')


if __name__ == '__main__':
    main()
