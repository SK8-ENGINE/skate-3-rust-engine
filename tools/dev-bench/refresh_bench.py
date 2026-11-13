"""Time a group-only Refresh Game Files on a bench copy of the installation and hash what it produced.

py -3.13 refresh_bench.py TREE BENCH_BASE GAME_EXE GROUP LABEL
  TREE        folder holding tools/ (pristine snapshot or the working tree)
  BENCH_BASE  a base like data/ (installation.json + installations/<id>), a real copy, never the user's
  GROUP       pipeline group to mark stale (hud, audio, ...)
Writes runs/<LABEL>.json: per-step seconds (from setup's progress lines) and the sha256 of every output file.
Idle priority (SKATE_SETUP_PRIORITY=idle); setup's own window is not used.
"""
import hashlib, json, os, sys, time
from pathlib import Path

tree, base, game_exe, group, label = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), Path(sys.argv[3]), sys.argv[4], sys.argv[5]
os.environ.setdefault('SKATE_SETUP_PRIORITY', 'idle')
import ctypes  # the whole run Idle (setup lowers itself only inside install(), not for the hashing after it)
ctypes.windll.kernel32.SetPriorityClass(ctypes.windll.kernel32.GetCurrentProcess(), 0x40)
os.environ.setdefault('SKATE_REPORT_CHILD', '1')
sys.path.insert(0, str(tree))
os.chdir(tree)
from tools.asset_pipeline.customiser_setup import install
from tools.asset_pipeline.versions import installed

marker = base/'installation.json'
data = json.loads(marker.read_text(encoding='utf-8'))
data['pipelines'][group] = 'stale'
marker.write_text(json.dumps(data, separators=(',', ':')), encoding='utf-8')
source = Path(data['source'])

start = last = time.perf_counter()
steps = []
def report(text):
    global last
    now = time.perf_counter()
    steps.append({'at': round(now-start, 2), 'since_last': round(now-last, 2), 'text': text})
    print(f'{now-start:8.1f}s (+{now-last:6.1f}) {text}', flush=True)
    last = now

root = install(source, base, game_exe, report, refresh=True)
total = time.perf_counter()-start
print(f'TOTAL {total:.1f}s', flush=True)

def sha(p):
    h = hashlib.sha256()
    with open(p, 'rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()
files = {p.relative_to(root).as_posix(): sha(p) for p in sorted(root.rglob('*')) if p.is_file()}
final = json.loads(marker.read_text(encoding='utf-8'))
final.pop('directory', None)
out = Path(__file__).parent/'runs'
out.mkdir(exist_ok=True)
(out/f'{label}.json').write_text(json.dumps({'label': label, 'tree': str(tree), 'group': group, 'total_s': round(total, 1),
    'steps': steps, 'files': files, 'marker': final}, indent=1), encoding='utf-8')
print(f'{len(files)} files hashed -> runs/{label}.json')
