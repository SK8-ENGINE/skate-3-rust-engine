"""Local owned-disc installation. No game content is downloaded or packaged."""
from pathlib import Path
import hashlib,json,os,re,shutil,subprocess,sys,time,urllib.request,uuid,zipfile
from concurrent.futures import ThreadPoolExecutor,as_completed
from tools.owned_game.big import BigArchive

TOOLS=Path(__file__).resolve().parents[1]

GIB=1024**3

def available_memory():
    """Available physical memory in bytes, or None where it cannot be queried."""
    if os.name!='nt':return None
    import ctypes
    class Memory(ctypes.Structure):
        _fields_=[('length',ctypes.c_ulong),('load',ctypes.c_ulong)]+[(name,ctypes.c_ulonglong) for name in
            ('total','available','page_total','page_available','virtual_total','virtual_available','extended')]
    memory=Memory();memory.length=ctypes.sizeof(memory)
    return memory.available if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(memory)) else None

def map_workers():
    count=min(3,max(1,(os.cpu_count() or 1)//2))
    available=available_memory()
    if available is not None:
        # Reserve memory for the desktop; each map and its loader can
        # briefly hold several copies of geometry and textures.
        count=min(count,max(1,(available-2*GIB)//(3*GIB)))
    return count

def overlap_customiser(workers):
    """Run the character customiser beside the map jobs only with room for one
    more 3 GiB slot on top of the map workers and the 2 GiB desktop reserve."""
    available=available_memory()
    return available is None or available>=2*GIB+(workers+1)*3*GIB

class Background:
    """Run an action on a thread; join() waits and re-raises its exception."""
    def __init__(self,action,name):
        import threading
        self.error=None
        def target():
            try:action()
            except BaseException as error:self.error=error
        self.thread=threading.Thread(target=target,name=name,daemon=True)
        self.thread.start()

    def join(self):
        self.thread.join()
        if self.error is not None:raise self.error
XISO_URL='https://github.com/XboxDev/extract-xiso/releases/download/build-202505152050/extract-xiso-Win64_Release.zip'
XISO_SHA='fec88d03c7efd6205ab09be4abba70c0afd0eb27a5709f0a6235b828ba5ac11e'

def digest(path):
    with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def remove_intermediate(path,root):
    target=path.resolve();root=root.resolve()
    if target==root or not target.is_relative_to(root):
        raise RuntimeError('Refusing to remove a path outside conversion workspace')
    shutil.rmtree(target)

_INSTALLATION_ID=re.compile(r'^[0-9a-f]{32}$')

def active_installation_id(base):
    marker=base/'installation.json'
    if not marker.is_file():
        return None
    try:
        directory=json.loads(marker.read_text(encoding='utf-8-sig')).get('directory')
    except (OSError,ValueError,TypeError):
        return None
    if not isinstance(directory,str) or not re.fullmatch(r'installations/[0-9a-f]{32}',directory):
        return None
    return directory.split('/',1)[1]

def setup_log_name(name):
    return (name == 'setup.log'
            or name.endswith('-conversion.log') or name.endswith('-load.log'))

def remove_setup_logs(stage, report=lambda _:None):
    for path in stage.iterdir():
        if path.is_file() and setup_log_name(path.name):
            report('Removing setup log '+path.name)
            path.unlink(missing_ok=True)

def remove_stale_installations(base, active_stage, report=lambda _:None):
    """Drop superseded installation trees after a successful publish."""
    base=base.resolve()
    installations=base/'installations'
    if not installations.is_dir():
        return
    active_id=active_installation_id(base)
    active_root=active_stage.resolve()
    if active_id is None or active_root!=(installations/active_id).resolve():
        return
    if not active_root.is_relative_to(base) or not active_root.is_dir():
        return
    for entry in installations.iterdir():
        if not entry.is_dir() or not _INSTALLATION_ID.fullmatch(entry.name):
            continue
        if entry.resolve()==active_root:
            continue
        report('Removing previous installation '+entry.name)
        remove_intermediate(entry,installations)

def download(url,expected,cache,report):
    cache.mkdir(parents=True,exist_ok=True)
    archive=cache/url.rsplit('/',1)[1]
    if not archive.is_file() or digest(archive)!=expected:
        report('Downloading '+archive.name)
        temp=archive.with_suffix('.part')
        request=urllib.request.Request(url,headers={'User-Agent':'Mozilla/5.0 Skate3RustEngine-Setup/1.0'})
        with urllib.request.urlopen(request,timeout=60) as response,temp.open('wb') as output:
            shutil.copyfileobj(response,output,1024*1024)
        if digest(temp)!=expected:raise RuntimeError('Download checksum mismatch: '+archive.name)
        temp.replace(archive)
    return archive

def unpack_zip(archive,destination):
    destination=destination.resolve()
    with zipfile.ZipFile(archive) as z:
        for info in z.infolist():
            target=(destination/info.filename).resolve()
            if not target.is_relative_to(destination):raise RuntimeError('Unsafe tool archive path')
            if (info.external_attr>>16)&0o170000==0o120000:raise RuntimeError('Tool archive contains a symbolic link')
        z.extractall(destination)

def dependency(cache,name,url,sha,report):
    folder=cache/name
    marker=folder/'.complete'
    if not marker.is_file():
        unpack_zip(download(url,sha,cache,report),folder)
        marker.write_text(sha)
    executable=next(folder.rglob(name+'.exe'),None)
    if executable is None:raise RuntimeError('Missing downloaded tool: '+name)
    return executable

def spawn(args,**popen):
    kwargs={'creationflags':subprocess.CREATE_NO_WINDOW} if os.name=='nt' else {}
    external=os.name=='nt' and getattr(sys,'frozen',False) and Path(args[0]).resolve()!=Path(sys.executable).resolve()
    if external:
        # External tools and the game must load their own libraries, not
        # the setup bundle's DLL directory inherited by child processes.
        # SetDllDirectoryW is process-wide: only spawn external tools from one thread.
        import ctypes
        ctypes.windll.kernel32.SetDllDirectoryW(None)
        env=os.environ.copy()
        bundle=Path(sys._MEIPASS).resolve()
        env['PATH']=os.pathsep.join(p for p in env.get('PATH','').split(os.pathsep)
                                   if p and not Path(p).resolve().is_relative_to(bundle))
        kwargs['env']=env
    try:
        return subprocess.Popen([str(a) for a in args],text=True,encoding='utf-8',errors='replace',**kwargs,**popen)
    finally:
        if external:ctypes.windll.kernel32.SetDllDirectoryW(sys._MEIPASS)

def run(args,log,report):
    child=spawn(args,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    with child as process:
        for line in process.stdout:
            log.write(line);log.flush()
        if process.wait():raise RuntimeError('Conversion failed. See '+str(log.name))

class MapValidator:
    """One `skate3rust --validate-maps` process (crates/skate-game/src/map_validation.rs).

    Shared stock data loads once, so each map costs well under a second instead
    of a full --check-assets launch. Requests are one path (or TEST_WORLD) per
    line; each answer is one `SKATE_MAP_CHECK {json}` stdout line. Game logs go
    to stderr and are copied to the setup log. Use from one thread only.
    """
    READY='SKATE_VALIDATOR_READY'
    RESULT='SKATE_MAP_CHECK '

    def __init__(self,args,log):
        import threading
        self.process=spawn(args,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.log=log;self.lock=threading.Lock()
        def copy_stderr():
            with self.process.stderr as stream:
                for line in stream:
                    with self.lock:log.write(line);log.flush()
        threading.Thread(target=copy_stderr,daemon=True).start()
        ready=self.process.stdout.readline()
        if not ready.startswith(self.READY):
            self.close()
            raise RuntimeError('Map validator did not start'+(f': {ready.strip()}' if ready.strip() else ''))
        self.write(ready)

    def write(self,line):
        with self.lock:self.log.write(line if line.endswith('\n') else line+'\n');self.log.flush()

    def check(self,request):
        """Return the result dict; RuntimeError if the validator is gone."""
        if '\n' in str(request) or '\r' in str(request):raise ValueError('Invalid map path')
        try:
            self.process.stdin.write(f'{request}\n');self.process.stdin.flush()
            while True:
                line=self.process.stdout.readline()
                if not line:break
                self.write(line)
                if line.startswith(self.RESULT):return json.loads(line[len(self.RESULT):])
        except (OSError,ValueError) as error:
            raise RuntimeError(f'Map validator failed: {error}') from error
        raise RuntimeError(f'Map validator exited (code {self.process.poll()})')

    def close(self):
        try:self.process.stdin.close()
        except OSError:pass  # already exited: the pipe is broken
        try:self.process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.process.kill();self.process.wait()
        self.process.stdout.close()

def start_validator(game_exe,assets,log,report):
    """MapValidator, or None (older game exe / start failure: use --check-assets)."""
    try:
        validator=MapValidator([game_exe,'--assets',assets,'--validate-maps'],log)
    except (OSError,RuntimeError) as error:
        report(f'Map validator unavailable ({error}); validating with --check-assets per map')
        return None
    import weakref
    # Never leave the process behind if setup fails before closing it.
    weakref.finalize(validator,validator.process.kill)
    return validator

def record_validation(private,entry,result,report):
    """Store non-blocking findings as map-status/<map>-validation.json (status warning)."""
    from .setup_state import atomic_json
    path=private/'map-status'/(entry['name']+'-validation.json')
    if result is None:return
    entry.setdefault('phase_seconds',{})['validate']=round(result['seconds'],3)
    if result['warnings']:
        path.parent.mkdir(parents=True,exist_ok=True)
        atomic_json(path,{'version':1,'component':entry['name']+' map checks','status':'warning',
                          'warnings':result['warnings']})
        report(f"{entry['name']}: map check warnings: {'; '.join(result['warnings'])}")
    else:
        path.unlink(missing_ok=True)

def task(script,*args):
    if getattr(sys,'frozen',False):return [sys.executable,'--task',str(script),*map(str,args)]
    return [sys.executable,str(script),*map(str,args)]

def extract(archive,destination,entries=None):
    data=BigArchive(archive)
    data.extract_entries(data.entries if entries is None else [e for e in data.entries if entries(e)],destination)
    return data

def convert_map(archive,work,maps,stage,game_exe,log,report):
    timings={};started=time.perf_counter()
    def finished(phase):
        nonlocal started
        now=time.perf_counter();timings[phase]=round(now-started,3);started=now
        report(f'{archive.stem}: {phase} {timings[phase]:.3f}s')
    map_tools=TOOLS/'vendor/university/tools/vanilla_map_extraction/tools'
    sys.path.insert(0,str(map_tools))
    from prepare_hawaiian_dream import prepare
    from prepare_university import EXCLUDED_NORMAL_TEXTURE_IDS
    from build_retail_collision_archive import build_archive
    from .map_writer import write as write_map, SpawnSelector
    district=archive.stem.removeprefix('world')
    label=district.removeprefix('DIST_')
    district_work=work/district
    extract(archive,district_work/'raw')
    finished('extract')
    stream=district_work/'raw/data/content/world/stream'/district
    if not stream.is_dir():raise RuntimeError('Missing district stream '+str(stream))
    spawn=SpawnSelector(district)
    manifest_path=prepare(stream_directory=stream,output_root=district_work/'intermediate',
        utt_root=TOOLS/'vendor/utt',district_name=district,map_name=label,
        package_name='Skate 3 owned disc',cache_format='skate3-rust-map-v1',
        # Smaller parks keep their textures in Pres rather than a Tex stream.
        texture_stream_names=('Tex',) if any(stream.glob('cTex_*.xsf')) else (),
        excluded_normal_texture_ids=EXCLUDED_NORMAL_TEXTURE_IDS,raw_texture_cache=True,
        collision_consumer=spawn.consider,
        # Model/texture RX2 copies are unused by the direct writer and were
        # deleted after conversion. Keep simulation and irradiance sources.
        write_render_sources=False)
    finished('prepare')
    collision=district_work/'collision.rwcmset'
    build_archive(manifest_path,collision)
    finished('collision_archive')
    final=maps/(label+'.skate')
    # Authored retail start (map_starts.py) when _install found one; else geometry.
    starts=work/'map-starts.json'
    start=json.loads(starts.read_text(encoding='utf-8')).get(district) if starts.is_file() else None
    if start:report(f"{label}: authored start {start['locator']} ({start['source']})")
    write_map(manifest_path,final,collision,report,
              prepared_spawn=tuple(start['position']) if start else spawn.result(label),
              prepared_heading=start['heading'] if start else 0.)
    finished('write_map')
    from .dynamic_props import export as write_props
    caches=list((work/'dmo/cache').glob('DMO_*'))
    from .optional_content import CONTENT_ERRORS, note
    props=stage/'assets/private/native-props'/(label+'.skate')
    try:
        if not caches:raise RuntimeError('Movable-object source catalog is unavailable')
        placed, unresolved=write_props(manifest_path,caches,props,catalog_path=work/'dmo/catalog.json')
        (stage/'assets/private/native-props'/(label+'-availability.json')).unlink(missing_ok=True)
    except CONTENT_ERRORS as error:
        if props.is_dir():remove_intermediate(props,stage)
        note(stage/'assets/private/native-props'/(label+'-availability.json'),label+' movable props',error,report=report)
        placed,unresolved=0,0
    finished('props')
    report(f'{label}: placed {placed} authored DMO instances, {unresolved} unresolved templates')
    # Validation runs in the parent (_install, MapValidator) as each map finishes.
    entry={'name':label,'path':'maps/'+final.name,'sha256':digest(final)}
    remove_intermediate(district_work,work)
    finished('hash_and_cleanup')
    entry['phase_seconds']=timings
    return entry


def install(iso,base,game_exe,report,game_root=None,refresh=False,finalize=None):
    from .setup_state import setup_lock
    with setup_lock(base):
        return _install(iso,base,game_exe,report,game_root,refresh,finalize)


def _install(iso,base,game_exe,report,game_root=None,refresh=False,finalize=None):
    from .versions import fingerprints, changed_groups, installed, GROUPS
    from .group_receipts import damaged, record
    from .setup_state import atomic_json
    from . import asset_exports as exports
    target_versions=fingerprints()
    previous=installed(base) if refresh else None
    groups=changed_groups(previous[1].get('pipelines',{}),target_versions) if previous else set(GROUPS)
    source=str((iso if iso is not None else game_root).resolve())
    if game_root is None and iso is not None:
        selected=iso.resolve()
        if selected.is_dir():game_root=selected
        elif selected.suffix.lower()=='.xex':
            if selected.name.lower()!='default.xex' or not selected.is_file():
                raise RuntimeError('Select default.xex inside your extracted Skate 3 game folder')
            game_root=selected.parent
    base=base.resolve();base.mkdir(parents=True,exist_ok=True)
    from .setup_state import source_directory
    if game_root is not None:
        game_root=source_directory(game_root, require_core=not previous)
        if previous and previous[1].get('source_hash') not in (None,digest(game_root/'default.xex')):
            raise RuntimeError('Select the same Xbox game edition used to set up this copy')
    if previous:groups.update(damaged(*previous, exclude=groups))
    def outputs(stage):
        saved=previous[1].get('outputs',{}) if previous else {}
        return {g:saved[g] if g not in groups and g in saved else record(stage,g) for g in GROUPS}
    if not groups:
        with (base/'refresh-validation.log').open('w',encoding='utf-8') as log:
            run([game_exe,'--assets',previous[0]/'assets','--test-world','--check-assets'],log,report)
        if finalize:finalize(previous[0])
        from .validation_report import summary
        summary(previous[0])
        atomic_json(base/'installation.json', {**previous[1], 'pipelines':target_versions,
                    'outputs':outputs(previous[0]), 'source':source})
        remove_setup_logs(previous[0],report)
        remove_stale_installations(base,previous[0],report)
        report('Game assets are current')
        return previous[0]
    install_id=uuid.uuid4().hex
    stage=base/'installations'/install_id
    stage.mkdir(parents=True)
    if previous:
        report('Preparing an asset update; keeping the previous installation until it succeeds')
        immutable_maps = ({(previous[0]/item['path']).resolve() for item in
                           json.loads((previous[0]/'maps.json').read_text())}
                          if 'maps' not in groups else set())
        from .customiser_cache import SOURCES as character_stages
        sets=previous[0]/'assets/private/customisation/sets'
        immutable_sets=[p.resolve() for p in sets.glob('*') if p.is_dir()
                        and all((p/(name+'-complete.json')).is_file() for name in character_stages)]
        for entry in previous[0].iterdir():
            if entry.name in {'conversion','setup-report.json'} or setup_log_name(entry.name):continue
            # Unchanged maps and immutable character generations share storage.
            # Mutable user data and rebuilt outputs get independent files.
            def copy_map(src,dst):
                if Path(src).resolve() in immutable_maps:
                    try:os.link(src,dst)
                    except OSError:shutil.copy2(src,dst)
                else:shutil.copy2(src,dst)
                return dst
            def copy_asset(src,dst):
                if any(Path(src).resolve().is_relative_to(root) for root in immutable_sets):
                    try:os.link(src,dst)
                    except OSError:shutil.copy2(src,dst)
                else:shutil.copy2(src,dst)
                return dst
            def ignore_rebuilt_assets(directory, names):
                # These raw inputs are owned by the converters. A rebuilding
                # group must extract into an empty cache, not overwrite files
                # copied from the previous installation. Leave that live copy
                # and all user settings/custom models untouched.
                source_directory = Path(directory).resolve()
                private_source = (previous[0]/'assets/private').resolve()
                if 'core' in groups and source_directory == private_source:
                    return {'stock'} & set(names)
                if 'character' in groups and source_directory == private_source/'stock/data/content':
                    return {'createacharacter'} & set(names)
                return set()
            if entry.is_dir():
                shutil.copytree(entry,stage/entry.name,
                    ignore=ignore_rebuilt_assets if entry.name=='assets' else None,
                    copy_function=copy_map if entry.name=='maps' and 'maps' not in groups
                    else copy_asset if entry.name=='assets' else shutil.copy2)
            else:shutil.copy2(entry,stage/entry.name)
    private=stage/'assets/private';private.mkdir(parents=True,exist_ok=True)
    maps=stage/'maps';maps.mkdir(exist_ok=True)
    work=stage/'conversion';work.mkdir()
    with (stage/'setup.log').open('w',encoding='utf-8') as log:
        if game_root is None:
            iso=iso.resolve()
            if not iso.is_file() or iso.suffix.lower()!='.iso':raise RuntimeError('Select an Xbox 360 Skate 3 ISO')
            extractor=dependency(base/'tools','extract-xiso',XISO_URL,XISO_SHA,report)
            game_root=work/'disc'
            report('Extracting your ISO')
            # extract-xiso expects all options before the ISO path.
            run([extractor,'-x','-d',game_root,iso],log,report)
        else:game_root=game_root.resolve()
        required_files=['default.xex']
        if 'core' in groups:required_files += ['data/big/miscload.big','data/big/miscboot.big','data/big/db.big']
        if 'character' in groups:required_files += ['data/content/createacharacter.big']
        for required in required_files:
            if not (game_root/required).is_file():raise RuntimeError('This is not a supported Skate 3 disc: missing '+required)
        source_hash=digest(game_root/'default.xex')
        if previous and previous[1].get('source_hash',source_hash)!=source_hash:
            raise RuntimeError('Select the same Xbox game edition used to set up this copy')
        stock=private/'stock'
        if 'core' in groups:
            converted=exports.core(game_root,stage,work,report,log)
        else:
            converted=json.loads((stock/'skater-collections.json').read_text(encoding='utf-8'))
        if 'hud' in groups:
            exports.hud(game_root,stage,work,report,log)
        if 'character' in groups:
            exports.character(game_root,stage,work,report,log,converted)
        if 'environment' in groups:
            exports.environment(game_root,stage,work,report,log,converted,game_exe)
        if 'audio' in groups:
            exports.audio(game_root,stage,work,report,log,base/'tools')
        if 'maps' in groups:
            report('Preparing authored movable-object models')
            from .dynamic_props import prepare_catalog
            from .optional_content import CONTENT_ERRORS, note
            try:
                prepare_catalog(game_root,work/'dmo')
                (private/'native-props/props-availability.json').unlink(missing_ok=True)
            except CONTENT_ERRORS as error:
                if (work/'dmo').exists():remove_intermediate(work/'dmo',work)
                note(private/'native-props/props-availability.json','Movable props',error,report=report)
        report('Validating skater, input and animation data')
        validator=start_validator(game_exe,stage/'assets',log,report)
        customiser=None  # Background customiser when overlapped with the map stage
        def validate(request):
            """Today's checks raise (map rejected); new findings come back as warnings."""
            nonlocal validator
            if validator is not None:
                try:result=validator.check(request)
                except RuntimeError as error:
                    report(f'{error}; validating with --check-assets per map')
                    validator.close();validator=None
                else:
                    if not result['ok']:raise RuntimeError('Map validation failed: '+'; '.join(result['errors']))
                    return result
            target=['--test-world'] if request=='TEST_WORLD' else ['--map',request]
            run([game_exe,'--assets',stage/'assets',*target,'--check-assets'],log,report)
            return None
        validate('TEST_WORLD')
        if 'maps' in groups:
            archives=list((game_root/'data/content').glob('worldDIST_*.big'))
            archives.sort(key=lambda p:(p.stem!='worldDIST_University',p.name.lower()))
            from .map_starts import prepare as map_starts
            try:
                starts=map_starts(game_root,converted)
            except CONTENT_ERRORS as error:
                starts={}
                report(f'Authored map starts unavailable ({error}); using geometric spawns')
            (work/'map-starts.json').write_text(json.dumps(starts),encoding='utf-8')
            workers=map_workers()
            # The customiser reads only stock, default-skater and disc data (all
            # prepared above) and writes only assets/private/customisation, so it
            # runs beside the map jobs instead of after them.
            if finalize and overlap_customiser(workers):
                report('Preparing the character customiser alongside the maps')
                customiser=Background(lambda:finalize(stage),'customiser')
            report(f'Converting {len(archives)} maps with {workers} workers')
            def map_job(archive):
                result=work/(archive.stem+'.json')
                with (stage/(archive.stem+'-conversion.log')).open('w',encoding='utf-8') as map_log:
                    run(task(TOOLS/'asset_pipeline/map_job.py','--archive',archive,'--stage',stage,
                             '--game-exe',game_exe,'--result',result),map_log,report)
                return json.loads(result.read_text(encoding='utf-8'))
            completed={}
            with ThreadPoolExecutor(max_workers=workers) as pool:
                # Start the expensive districts together so one does not
                # remain queued behind a string of small parks.
                futures={pool.submit(map_job,a):a for a in sorted(archives,key=lambda p:-p.stat().st_size)}
                for future in as_completed(futures):
                    archive=futures[future]
                    try:
                        completed[archive.name]=future.result()
                        # Validate while the remaining maps keep converting.
                        checked=validate(stage/completed[archive.name]['path'])
                    except CONTENT_ERRORS as error:
                        completed.pop(archive.name,None)
                        label=archive.stem.removeprefix('worldDIST_')
                        (maps/(label+'.skate')).unlink(missing_ok=True)
                        props=private/'native-props'/(label+'.skate')
                        if props.is_dir():remove_intermediate(props,private)
                        note(private/'map-status'/(label+'-availability.json'),label,error,report=report)
                        continue
                    (private/'map-status'/(completed[archive.name]['name']+'-availability.json')).unlink(missing_ok=True)
                    record_validation(private,completed[archive.name],checked,report)
                    report(f"Converted {len(completed)}/{len(archives)} maps: {completed[archive.name]['name']}")
            catalog=[completed[a.name] for a in archives if a.name in completed]
            if previous:
                # An absent/failed source district may still have a usable old
                # converted copy. Validate its bytes AND load with this engine.
                for old in json.loads((previous[0]/'maps.json').read_text()):
                    if any(item['path']==old['path'] for item in catalog):continue
                    src=(previous[0]/old['path']).resolve()
                    if not src.is_relative_to((previous[0]/'maps').resolve()):raise ValueError('Invalid old map path')
                    if not src.is_file() or digest(src)!=old.get('sha256'):continue
                    try:validate(src)
                    except CONTENT_ERRORS:continue
                    target=stage/old['path'];target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(src,target)
                    catalog.append(old)
                    note(private/'map-status'/(old['name']+'-availability.json'),old['name'],
                         RuntimeError('New source map unavailable; previous map passed validation'),retained=True,report=report)
            # Exclude invalid old retail maps from the runtime directory scan.
            valid_paths={item['path'] for item in catalog}
            if previous:
                for old in json.loads((previous[0]/'maps.json').read_text()):
                    if old['path'] not in valid_paths:(stage/old['path']).unlink(missing_ok=True)
            if not catalog:raise RuntimeError('No playable map could be prepared or recovered. Restore at least one worldDIST_*.big archive beside default.xex and retry; the previous installation has been kept.')
        report('Validating installed runtime inputs')
        validate('TEST_WORLD')
        if validator is not None:validator.close()
        settings=stage/'settings';settings.mkdir(exist_ok=True)
        if not previous:
            (settings/'default-map.json').write_text(json.dumps(next((m['path'] for m in catalog if m['name']=='University'),catalog[0]['path'])),encoding='utf-8')
        if 'maps' in groups:
            (stage/'maps.json').write_text(json.dumps(catalog,indent=2),encoding='utf-8')
            selected=settings/'default-map.json'
            if selected.is_file() and not (stage/json.loads(selected.read_text())).is_file():
                selected.write_text(json.dumps(catalog[0]['path']))
        remove_intermediate(work,stage)
        if customiser is not None:customiser.join()
        elif finalize:finalize(stage)
        from .validation_report import summary
        warnings=summary(stage)
    # Publish after core validation and all optional outcomes have been recorded.
    marker=base/'installation.json.new'
    marker.write_text(json.dumps({'version':1,'directory':'installations/'+install_id,'source':source,'source_hash':source_hash,'pipelines':target_versions,'outputs':outputs(stage)}),encoding='utf-8')
    marker.replace(base/'installation.json')
    remove_setup_logs(stage,report)
    remove_stale_installations(base,stage,report)
    report(f'Setup complete ({len(warnings)} warnings or unavailable/retained components; see setup-report.json)' if warnings else 'Setup complete')
    return stage
