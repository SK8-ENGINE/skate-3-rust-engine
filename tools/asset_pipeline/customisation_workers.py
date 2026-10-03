"""Parallel worker processes for the character customiser (source and frozen setup).

Two jobs, each reading a JSON request file:
  --warm-textures REQUEST  decode clothing-library textures into the decoded/ cache
                           (customisation_library.warm_textures)
  --roster REQUEST         prepare a subset of pro/special characters
                           (native_roster.prepare with only=..., report_path=...)
Workers only fill caches or write disjoint per-character outputs; the callers
merge results in the original order, so outputs match a serial run.
"""
from pathlib import Path
import json, os, sys
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))


def customiser_workers():
    """Bounded share of the machine: a quarter of logical CPUs, 1..8 processes."""
    return max(1, min(8, (os.cpu_count() or 2) // 4))


def run_parallel(arguments, workers):
    """Run `customisation_workers.py <args>` processes, at most `workers` at once.

    Uses install.run/task so frozen setup builds re-enter through --task.
    Returns one exception-or-None per argument list, in order.
    """
    import tempfile
    from concurrent.futures import ThreadPoolExecutor
    from tools.asset_pipeline.install import run, task
    with tempfile.TemporaryDirectory(prefix='customiser-workers-') as tmp:
        def job(index):
            path = Path(tmp)/f'{index}.log'
            with path.open('w', encoding='utf-8') as log:
                try:
                    run(task(Path(__file__), *arguments[index]), log, lambda _: None)
                    return None
                except RuntimeError as error:
                    failure = error
            # The log directory is deleted on return; keep the cause visible.
            tail = path.read_text(encoding='utf-8', errors='replace').splitlines()[-8:]
            print(f'Customiser worker {index} failed: {failure}', *tail, sep='\n  ', flush=True)
            return RuntimeError(f'{failure}: ' + ' | '.join(tail))
        with ThreadPoolExecutor(max_workers=max(1, workers)) as pool:
            return list(pool.map(job, range(len(arguments))))


def main(argv):
    mode, request = argv[0], json.loads(Path(argv[1]).read_text(encoding='utf-8'))
    if mode == '--warm-textures':
        from tools.asset_pipeline.customisation_library import warm_textures
        warm_textures(request['config'], request['textures'])
    elif mode == '--roster':
        from tools.asset_pipeline.native_roster import prepare
        prepare(*(Path(request[k]) for k in ('game', 'assets', 'library', 'collections', 'work')),
                only=request['only'], report_path=Path(request['report']))
    else:
        raise SystemExit(f'Unknown customiser worker mode {mode}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
