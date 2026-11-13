"""One in-process setup stage in its own process, so it runs beside another stage (install.py; doc 08 "Export speed
pass"). The stage function is the same as in-process; its progress goes to stdout as `REPORT <text>` lines and its log
lines as they are (the parent copies them into the setup log)."""
from pathlib import Path
import argparse,sys
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))

if __name__=='__main__':
    p=argparse.ArgumentParser()
    p.add_argument('name',choices=['livingworld','customiser'])
    for name in ('game','stage','work'):p.add_argument('--'+name,type=Path,required=True)
    a=p.parse_args()
    from tools.asset_pipeline import asset_exports
    report=lambda text:print('REPORT '+text,flush=True)
    if a.name=='customiser':
        from tools.asset_pipeline.customiser_setup import prepare
        prepare(a.game,a.stage/'assets',report)
    else:getattr(asset_exports,a.name)(a.game,a.stage,a.work,report,sys.stdout)
    sys.stdout.flush()
