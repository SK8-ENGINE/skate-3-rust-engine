"""Side data of DLC maps kept from the previous installation: docs/hails-additions/38-dlc-support.md#kept-dlc-maps-keep-their-side-data"""
from pathlib import Path
import json,shutil


def _load(path):
    try:return json.loads(Path(path).read_text(encoding='utf-8'))
    except (OSError,ValueError):return None


def _write(path,data,indent):
    # A new file swapped in: the old one may be a hard link shared with the previous installation (install.py).
    path.parent.mkdir(parents=True,exist_ok=True)
    temporary=path.with_name(path.name+'.tmp')
    temporary.write_text(json.dumps(data,indent=indent),encoding='utf-8')
    temporary.replace(path)


def _copy(src,dst):
    if src.is_file() and not dst.exists():
        dst.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(src,dst)
        return 1
    return 0


def kept_dlc_maps(previous_catalog,disc_labels,maps_dir):
    """Names of the previous installation's maps without a disc archive that are still installed."""
    disc={label.lower() for label in disc_labels}
    return {m['name'] for m in previous_catalog
            if m['name'].lower() not in disc and (Path(maps_dir)/Path(m['path']).name).is_file()}


def _rows(old_path,new_path,key,maps,indent):
    """Adds the old file's `key` rows of `maps` whose id the new file lacks."""
    old=_load(old_path)
    if not old or not isinstance(old.get(key),list):return 0
    new=_load(new_path) or {**{k:v for k,v in old.items() if k!=key},key:[]}
    have={row.get('id') for row in new.get(key,[])}
    add=[row for row in old[key] if row.get('map') in maps and row.get('id') not in have]
    if add:
        new[key]=new.get(key,[])+add
        _write(Path(new_path),new,indent)
    return len(add)


def _stream_map(stream):
    return stream[5:] if stream.lower().startswith('dist_') else stream


def _park_layouts(old_dir,new_dir,maps):
    old=_load(old_dir/'worlds.json')
    if not old:return 0
    new=_load(new_dir/'worlds.json') or {'version':old.get('version',1),'worlds':{}}
    lower={m.lower() for m in maps}
    add={k:v for k,v in old.get('worlds',{}).items()
         if k not in new['worlds'] and _stream_map(v.get('stream','')).lower() in lower}
    for layout in sorted({v['layout'] for v in add.values()}):
        for f in sorted(old_dir.glob(layout+'.*')):
            if f.name.split('.')[0]==layout:_copy(f,new_dir/f.name)
    if add:
        new['worlds']=dict(sorted({**new['worlds'],**add}.items()))
        _write(new_dir/'worlds.json',new,2)
    return len(add)+_rows(old_dir/'teleporters.json',new_dir/'teleporters.json','teleporters',maps,1)


def _files_of(value):
    if isinstance(value,dict):
        for k,v in value.items():
            if k=='file' and isinstance(v,str):yield v
            else:yield from _files_of(v)
    elif isinstance(value,list):
        for v in value:yield from _files_of(v)


def _audio(old_audio,new_audio):
    """Adds the old manifest's entries the new one lacks (dict sections only), with their files."""
    old=_load(old_audio/'audio_manifest.json');new=_load(new_audio/'audio_manifest.json')
    if not old or not new:return 0
    added=0
    for section,entries in old.items():
        if not isinstance(entries,dict) or not isinstance(new.get(section),dict):continue
        for key,entry in entries.items():
            if key in new[section]:continue
            new[section][key]=entry
            added+=1
            for file in _files_of(entry):_copy(old_audio/file,new_audio/file)
            if section=='bank_packs' and isinstance(entry,str):_copy(old_audio/entry,new_audio/entry)
    if added:_write(new_audio/'audio_manifest.json',new,None)
    return added


def _unlocks(old_path,new_path):
    old=_load(old_path)
    if not old or not old.get('unlocks'):return 0
    new=_load(new_path) or {'version':old.get('version',1),'unlocks':[]}
    have={json.dumps(u,sort_keys=True) for u in new['unlocks']}
    add=[u for u in old['unlocks'] if json.dumps(u,sort_keys=True) not in have]
    if add:
        new['unlocks']=new['unlocks']+add
        _write(Path(new_path),new,1)
    return len(add)


def carry(old_private,new_private,maps,report=lambda _:None):
    """After a refresh without the DLC folder: copy the kept DLC maps' side data from the previous installation's
    `assets/private` into the new one. Never replaces what this run wrote."""
    old_private,new_private=Path(old_private),Path(new_private)
    if not maps:return {}
    counts={
        'teleporters':_rows(old_private/'teleporters.json',new_private/'teleporters.json','teleporters',maps,1),
        'teleports':_rows(old_private/'teleports.json',new_private/'teleports.json','destinations',maps,2),
        'challenges':_rows(old_private/'challenges.json',new_private/'challenges.json','challenges',maps,1),
        'park layouts':_park_layouts(old_private/'park-layouts',new_private/'park-layouts',maps),
        'collection rows':sum(_copy(f,new_private/'stock/dlc'/f.name)
                              for f in sorted((old_private/'stock/dlc').glob('*')) if f.is_file()),
        'unlocks':_unlocks(old_private/'dlc-unlocks.json',new_private/'dlc-unlocks.json'),
        'audio entries':_audio(old_private/'audio',new_private/'audio'),
    }
    report('Kept DLC maps ('+', '.join(sorted(maps))+'): side data kept from the previous installation: '
           +', '.join(f'{k} {v}' for k,v in counts.items()))
    return counts
