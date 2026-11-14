"""Parallel worker processes for the character customiser (source and frozen setup).

`run_shared` starts worker processes that claim batches of one shared request until none are left, so workers started
later (when the maps finish and their cores are free, setup_budget.customiser_limit) join the same work. Batch kinds:
  textures  decode clothing-library textures into the decoded/ cache (customisation_library.warm_textures)
  models    convert clothing-library models into library/*_v4.glb (customisation_library.warm_models)
  derive    normal / rough maps and alpha-masked diffuses into library/textures (customisation_library.derive_textures)
  roster    prepare pro/special characters (native_roster.prepare with only=..., report_path=...)
Workers only fill caches or write disjoint per-character outputs; the callers
merge results in the original order, so outputs match a serial run.
"""
from pathlib import Path
import json, os, sys
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))


def customiser_workers():
    """Bounded share of the machine (setup_budget.customiser_workers: a quarter of the setup's core budget, 1..8)."""
    from tools.asset_pipeline.setup_budget import customiser_workers as budget
    return budget()


def batches(kind, items, size):
    return [{'kind': kind, 'items': items[i:i + size]} for i in range(0, len(items), size)]


def run_shared(payload, work, workers, poll=0.05):
    """Run worker processes over `work` (a list of batches), `workers` at once and more once
    setup_budget.customiser_limit allows. Each batch is claimed by exactly one worker (a file created exclusively).

    Uses install.run/task so frozen setup builds re-enter through --task. Returns (errors, claimed batch indices);
    a batch a failed worker claimed may be unfinished, which the callers' serial pass completes."""
    import tempfile, threading, time
    from tools.asset_pipeline.install import run, task
    from tools.asset_pipeline.setup_budget import customiser_limit
    if not work:
        return [], set()
    with tempfile.TemporaryDirectory(prefix='customiser-workers-') as tmp:
        claims = Path(tmp)/'claims';claims.mkdir()
        request = Path(tmp)/'request.json'
        request.write_text(json.dumps({**payload, 'batches': work, 'claims': str(claims)}), encoding='utf-8')
        errors, threads = [], []
        def job(index):
            path = Path(tmp)/f'{index}.log'
            with path.open('w', encoding='utf-8') as log:
                try:
                    run(task(Path(__file__), '--shared', request), log, lambda _: None)
                    return
                except RuntimeError as error:
                    failure = error
            # The log directory is deleted on return; keep the cause visible.
            tail = path.read_text(encoding='utf-8', errors='replace').splitlines()[-8:]
            print(f'Customiser worker {index} failed: {failure}', *tail, sep='\n  ', flush=True)
            errors.append(RuntimeError(f'{failure}: ' + ' | '.join(tail)))
        while True:
            running = sum(t.is_alive() for t in threads)
            unclaimed = len(work) - sum(1 for _ in claims.iterdir())
            # A failure stops new starts: a worker that fails at once must not be restarted for every batch.
            if unclaimed > 0 and not errors and len(threads) < len(work):
                limit = min(customiser_limit(workers, running), unclaimed + running)
                for _ in range(limit - running):
                    thread = threading.Thread(target=job, args=(len(threads),), daemon=True)
                    threads.append(thread);thread.start()
                    running += 1
            elif running == 0:
                break
            time.sleep(poll)
        claimed = {int(p.name) for p in claims.iterdir()}
    return errors, claimed


def claimed(request):
    """The batch indices this worker claims, one at a time, until every batch is taken."""
    claims = Path(request['claims'])
    for index in range(len(request['batches'])):
        try:
            os.close(os.open(claims/str(index), os.O_CREAT | os.O_EXCL | os.O_WRONLY))
        except FileExistsError:
            continue
        yield index


def shared(request):
    convert = None
    for index in claimed(request):
        batch = request['batches'][index]
        kind, items = batch['kind'], batch['items']
        if kind == 'textures':
            from tools.asset_pipeline.customisation_library import warm_textures
            warm_textures(request['config'], items)
        elif kind == 'models':
            from tools.asset_pipeline.customisation_library import warm_models, ModelConverter
            convert = convert or ModelConverter(request['config'])
            warm_models(request['config'], items, convert)
        elif kind == 'derive':
            from tools.asset_pipeline.customisation_library import derive_textures
            derive_textures(request['config'], items)
        elif kind == 'roster':
            from tools.asset_pipeline.native_roster import prepare
            prepare(*(Path(request[k]) for k in ('game', 'assets', 'library', 'collections', 'work')),
                    only=items, report_path=Path(request['reports'])/f'{index}.json')
        else:
            raise SystemExit(f'Unknown customiser batch kind {kind}')


def main(argv):
    mode, request = argv[0], json.loads(Path(argv[1]).read_text(encoding='utf-8'))
    if mode != '--shared':
        raise SystemExit(f'Unknown customiser worker mode {mode}')
    shared(request)
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
