"""Dump the decoded action streams, frame labels, imports, exports and character ids of retail APT movies as JSON for the Rust VM tests
(apt_vm.rs `retail_menu_movies_use_no_unknown_opcode`). Output is derived from retail data: keep it under
.local/, never commit it.

usage: python apt_actions_json.py <fedata.big> <out_dir> <data/fe/source/movie> [...]
"""
import json, sys, tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO / 'tools' / 'vendor'))
from skate3_ui.big import BigArchive  # noqa: E402
from skate3_ui.apt import AptInspector  # noqa: E402
from skate3_ui.actions import Actions  # noqa: E402


def main():
    big, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    a = BigArchive(big)
    ents = {e.name if hasattr(e, 'name') else e.path: e for e in a._parse()}
    for name in sys.argv[3:]:
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            for ext in ('apt', 'const'):
                (tmp / f'm.{ext}').write_bytes(a.read(ents[f'{name}.{ext}']))
            info = AptInspector(tmp / 'm.apt', tmp / 'm.const').inspect()
            acts = Actions((tmp / 'm.apt').read_bytes(), (tmp / 'm.const').read_bytes())
        chars = info['characters'] if isinstance(info['characters'], list) else info['characters'].values()
        streams, labels = {}, []
        for c in chars:
            for f in c.get('frames', []) or []:
                for ctl in f.get('controls', []):
                    kind = ctl.get('type_name')
                    if kind == 'frame_label':
                        labels.append(next((v for k, v in ctl.items() if isinstance(v, str) and k != 'type_name'), ''))
                    off = ctl.get('actions_offset', 0)
                    if kind in ('do_action', 'do_init_action') and off:
                        streams[f"{c.get('id')}_{kind}_{off:x}"] = acts.stream(off)
        target = out / (name.rsplit('/', 1)[-1] + '.json')
        target.write_text(json.dumps({'movie': name, 'actions': streams, 'labels': labels,
            'imports': [{k: i[k] for k in ('file', 'name', 'character_id')} for i in info['imports']],
            'exports': [{k: e[k] for k in ('name', 'character_id')} for e in info['exports']],
            'characters': sorted(c.get('id') for c in chars)}), encoding='utf-8')
        print(f'{name}: {len(streams)} streams, {len(labels)} labels -> {target}')


if __name__ == '__main__':
    main()
