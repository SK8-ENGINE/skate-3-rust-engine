"""Check B (skip the re-hash when size + mtime match) on the bench installation with the working tree's tools.

py -3.13 rehash_check.py TREE BENCH_BASE
Uses group_receipts.damaged() directly (no rebuild). Restores the one file it changes (bytes and time).
"""
import json, os, shutil, sys, time
from pathlib import Path

tree, base = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
sys.path.insert(0, str(tree))
from tools.asset_pipeline.versions import installed
from tools.asset_pipeline.group_receipts import damaged

root, marker = installed(base)
audio = marker['outputs']['audio']
timed = sum('mtime_ns' in item for item in audio.values())
print(f'audio receipt: {len(audio)} files, {timed} with mtime_ns')

def check(label, expect):
    _, fresh = installed(base)
    start = time.perf_counter()
    result = damaged(root, fresh)
    took = time.perf_counter() - start
    ok = result == expect
    print(f'{"PASS" if ok else "FAIL"} {label}: damaged={sorted(result)} expected={sorted(expect)} in {took:.1f}s')
    return ok, fresh

results = []
ok, _ = check('untouched installation', set())
results.append(ok)

name = max(audio, key=lambda n: audio[n]['size'] if audio[n]['size'] < 50_000_000 else 0)
target = root/name
backup = Path(__file__).parent/'runs'/'rehash-backup.bin'
shutil.copy2(target, backup)
before = os.stat(target)
print(f'test file: {name} ({before.st_size} bytes)')
try:
    # Same size, one byte flipped, the time moves as any write does: must be hashed and caught.
    data = bytearray(target.read_bytes())
    data[len(data) // 2] ^= 0xFF
    target.write_bytes(data)
    ok, _ = check('one byte changed (same size, new time)', {'audio'})
    results.append(ok)
    # Same bytes again but a new time: hashed, still valid.
    shutil.copyfile(backup, target)
    ok, fresh = check('original bytes, new time', set())
    results.append(ok)
    entry = fresh['outputs']['audio'][name]
    results.append(entry.get('mtime_ns') == os.stat(target).st_mtime_ns)
    print(f'{"PASS" if results[-1] else "FAIL"} the checked entry took the new time (in memory)')
finally:
    shutil.copyfile(backup, target)
    os.utime(target, ns=(before.st_atime_ns, before.st_mtime_ns))
    backup.unlink()
after = os.stat(target)
results.append((after.st_size, after.st_mtime_ns) == (before.st_size, before.st_mtime_ns))
print(f'{"PASS" if results[-1] else "FAIL"} test file restored (size and time)')
ok, _ = check('restored installation', set())
results.append(ok)
print('ALL PASS' if all(results) else 'SOME FAILED')
