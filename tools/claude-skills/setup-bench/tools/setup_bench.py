"""Timed full setup into a scratch base, for setup speed work (skill optimisation; doc 08).

Usage (from a checkout root, so `tools` is that checkout's pipeline):
    py -3.13 <this> --checkout <repo checkout> --base <scratch base> --game-exe <skate3rust.exe> --source <default.xex> --label <name>

Writes next to the base:
    <label>-timing.log   seconds since start, then each setup progress line
    <label>-hashes.tsv   sha256 and size of every file under the installed assets and maps (relative path, sorted)
The base is deleted first when it exists and holds a previous bench (marker file BENCH).
"""
import argparse
import hashlib
import os
import shutil
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def manifest(root, out):
    files = sorted(p for p in root.rglob('*') if p.is_file())
    with ThreadPoolExecutor(8) as pool:
        digests = list(pool.map(sha256, files))
    with open(out, 'w', encoding='utf-8') as f:
        for p, d in zip(files, digests):
            f.write(f'{p.relative_to(root).as_posix()}\t{p.stat().st_size}\t{d}\n')
    return len(files)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--checkout', type=Path, required=True)
    ap.add_argument('--base', type=Path, required=True)
    ap.add_argument('--game-exe', type=Path, required=True)
    ap.add_argument('--source', type=Path, required=True)
    ap.add_argument('--label', required=True)
    ap.add_argument('--no-hash', action='store_true')
    ap.add_argument('--python', type=Path, help='a Python with numpy and Pillow (e.g. .local/venv-setup of a checkout) when this one has none')
    a = ap.parse_args()
    try:
        import numpy, PIL  # noqa: F401
    except ImportError:
        # A system Python without the setup's packages (laptop 2026-10-24): rerun in a setup venv.
        roots = [a.checkout, *Path(__file__).resolve().parents]
        venvs = [v for r in roots for v in (r / '.local/venv-setup', r / '.local/versions/hails-additions/.local/venv-setup')]
        for python in ([a.python] if a.python else []) + [v / 'Scripts' / 'python.exe' for v in venvs]:
            if python.is_file() and Path(sys.executable).resolve() != python.resolve():
                import subprocess
                print(f'setup_bench: numpy / Pillow missing here, running with {python}', flush=True)
                sys.exit(subprocess.call([str(python), str(Path(__file__).resolve()), *sys.argv[1:]]))
        sys.exit('setup_bench: numpy / Pillow missing; run tools/setup.py once (it creates .local/venv-setup) or pass --python <a checkout>/.local/venv-setup/Scripts/python.exe')
    if a.base.exists():
        if not (a.base / 'BENCH').exists():
            sys.exit(f'{a.base} exists and is not a bench base')
        shutil.rmtree(a.base)
    a.base.mkdir(parents=True)
    (a.base / 'BENCH').write_text('setup bench scratch base\n')
    sys.path.insert(0, str(a.checkout.resolve()))
    os.chdir(a.checkout)
    from tools.asset_pipeline.customiser_setup import install
    log_path = a.base.parent / f'{a.label}-timing.log'
    start = time.perf_counter()
    with open(log_path, 'w', encoding='utf-8') as log:
        def report(text):
            log.write(f'{time.perf_counter() - start:9.2f}\t{text}\n')
            log.flush()
        root = install(a.source, a.base, a.game_exe, report)
        report(f'DONE {root}')
    total = time.perf_counter() - start
    print(f'{a.label}: setup {total:.1f} s, log {log_path}')
    if not a.no_hash:
        n = manifest(Path(root), a.base.parent / f'{a.label}-hashes.tsv')
        print(f'{a.label}: {n} files hashed')


if __name__ == '__main__':
    main()
