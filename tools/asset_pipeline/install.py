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
    # The districts are not equal: DownTown alone sets the map stage's length,
    # so more than three workers barely helps (SKATE_SETUP_MAP_WORKERS overrides).
    from .setup_budget import cores
    count=min(3,max(1,cores()//2))
    available=available_memory()
    if available is not None:
        # Reserve memory for the desktop; each map and its loader can
        # briefly hold several copies of geometry and textures.
        count=min(count,max(1,(available-2*GIB)//(3*GIB)))
    from .setup_budget import map_workers as budget
    return budget(count)

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
# Callers pass the Windows pins; other platforms use the same releases' builds.
# extract-xiso publishes no digests, so its pin is the downloaded archive's SHA-256.
PLATFORM_TOOLS={'linux':{
    'extract-xiso':('https://github.com/XboxDev/extract-xiso/releases/download/build-202505152050/extract-xiso_Linux.zip',
                    '982bbfefc9255d51f5348a477d7135d68abf81c0af9600e5728edb1246cfa200'),
    'vgmstream-cli':('https://github.com/vgmstream/vgmstream/releases/download/r2117/vgmstream-linux.zip',
                     '2f98c77f756079f63fbd119939067f1ed461d77e70993bc4cc372736d859c84a'),
}}

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

def dependency(cache,name,url,sha,report,windows=os.name=='nt'):
    # Windows: the pinned download. Elsewhere: the tool on PATH (Nix shell, distro package), else the pinned build
    # for this platform.
    if not windows:
        executable=shutil.which(name)
        if executable is not None:return Path(executable)
        if name not in PLATFORM_TOOLS.get(sys.platform,{}):
            raise RuntimeError(f'Missing tool: {name} is not on PATH and no {name} build is pinned for {sys.platform} (install it or run setup inside `nix develop`)')
        url,sha=PLATFORM_TOOLS[sys.platform][name]
    folder=cache/name
    marker=folder/'.complete'
    if not marker.is_file():
        unpack_zip(download(url,sha,cache,report),folder)
        marker.write_text(sha)
    executable=next((p for p in folder.rglob(name+'.exe' if os.name=='nt' else name) if p.is_file()),None)
    if executable is None:raise RuntimeError('Missing downloaded tool: '+name)
    # Zip archives do not keep the executable bit.
    if os.name!='nt':executable.chmod(executable.stat().st_mode|0o111)
    return executable

import threading
_SPAWN=threading.Lock()

def spawn(args,**popen):
    # One child start at a time: the frozen build's DLL directory switch below is process-wide, and the HUD, audio and
    # map stages start children from their own threads.
    with _SPAWN:
        return _spawn(args,**popen)

def _spawn(args,**popen):
    from .setup_budget import priority_class
    # Child processes inherit the setup priority budget.
    kwargs={'creationflags':subprocess.CREATE_NO_WINDOW|priority_class()} if os.name=='nt' else {}
    external=os.name=='nt' and getattr(sys,'frozen',False) and Path(args[0]).resolve()!=Path(sys.executable).resolve()
    if external:
        # External tools and the game must load their own libraries, not
        # the setup bundle's DLL directory inherited by child processes.
        # SetDllDirectoryW is process-wide: `spawn` holds _SPAWN around this.
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

def run_stage(name,game_root,stage,work,log,report,dlc=None):
    """An in-process stage function in its own process (stage_job.py): its `REPORT` lines go to `report`, the rest to
    the log, as when it ran here. `dlc`: the player's DLC folder (the living world reads the DLC maps' districts)."""
    child=spawn(task(TOOLS/'asset_pipeline/stage_job.py',name,'--game',game_root,'--stage',stage,'--work',work,*(('--dlc',dlc) if dlc else ())),
                stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    with child as process:
        for line in process.stdout:
            if line.startswith('REPORT '):report(line[7:].rstrip('\n'))
            else:log.write(line);log.flush()
        if process.wait():raise RuntimeError(f'Setup stage {name} failed. See '+str(log.name))

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

def convert_map(archive,work,maps,stage,game_exe,log,report,district=None,label=None,source=None):
    """One map from `archive`. Disc maps: worldDIST_<D>.big gives the stream folder and label. DLC maps name both
    (one pack BIG can hold several districts) and their source pack (research b415)."""
    timings={};started=time.perf_counter()
    name=label or archive.stem
    def finished(phase):
        nonlocal started
        now=time.perf_counter();timings[phase]=round(now-started,3);started=now
        report(f'{name}: {phase} {timings[phase]:.3f}s')
    map_tools=TOOLS/'vendor/university/tools/vanilla_map_extraction/tools'
    sys.path.insert(0,str(map_tools))
    from prepare_hawaiian_dream import prepare
    from prepare_university import EXCLUDED_NORMAL_TEXTURE_IDS
    from build_retail_collision_archive import build_archive
    from .map_writer import write as write_map, SpawnSelector
    district=district or archive.stem.removeprefix('world')
    label=label or district.removeprefix('DIST_')
    district_work=work/district
    # A DLC pack's BIG also holds its menus, audio and other districts: only this district's stream is needed.
    folder=('data/content/world/stream/'+district+'/').lower()
    extract(archive,district_work/'raw',(lambda e:e.path.replace('\\','/').lower().startswith(folder)) if source else None)
    finished('extract')
    stream=district_work/'raw/data/content/world/stream'/district
    if not stream.is_dir():raise RuntimeError('Missing district stream '+str(stream))
    spawn=SpawnSelector(district)
    manifest_path=prepare(stream_directory=stream,output_root=district_work/'intermediate',
        utt_root=TOOLS/'vendor/utt',district_name=district,map_name=label,
        package_name='Skate 3 owned DLC' if source else 'Skate 3 owned disc',cache_format='skate3-rust-map-v1',
        # Smaller parks keep their textures in Pres rather than a Tex stream.
        texture_stream_names=('Tex',) if any(stream.glob('cTex_*.xsf')) else (),
        excluded_normal_texture_ids=EXCLUDED_NORMAL_TEXTURE_IDS,raw_texture_cache=True,
        collision_consumer=spawn.consider,
        # Model/texture RX2 copies are unused by the direct writer and were
        # deleted after conversion. Keep simulation and irradiance sources.
        write_render_sources=False)
    finished('prepare')
    final=maps/(label+'.skate')
    # Authored retail start (map_starts.py) when _install found one; else geometry.
    starts=work/'map-starts.json'
    start=json.loads(starts.read_text(encoding='utf-8')).get(district) if starts.is_file() else None
    if start:report(f"{label}: authored start {start['locator']} ({start['source']})")
    from .dynamic_props import export as write_props
    caches=list((work/'dmo/cache').glob('DMO_*'))
    from .optional_content import CONTENT_ERRORS, note
    # Named trigger volumes (0x00EB0019) beside the map, like .irradiance.
    # Optional: a map without the sidecar simply has no trigger volumes.
    from .map_volumes import export as write_triggers
    triggers=final.with_suffix('.triggers')
    try:
        volumes=write_triggers(stream,triggers,label)
        (stage/'assets/private/map-status'/(label+'-triggers-availability.json')).unlink(missing_ok=True)
        report(f'{label}: {len(volumes)} trigger volumes')
    except CONTENT_ERRORS as error:
        triggers.unlink(missing_ok=True)
        note(stage/'assets/private/map-status'/(label+'-triggers-availability.json'),label+' trigger volumes',error,report=report)
    finished('triggers')
    props=stage/'assets/private/native-props'/(label+'.skate')
    def movable_props():
        try:
            if not caches:raise RuntimeError('Movable-object source catalog is unavailable')
            placed, unresolved=write_props(manifest_path,caches,props,catalog_path=work/'dmo/catalog.json')
            (stage/'assets/private/native-props'/(label+'-availability.json')).unlink(missing_ok=True)
        except CONTENT_ERRORS as error:
            if props.is_dir():remove_intermediate(props,stage)
            note(stage/'assets/private/native-props'/(label+'-availability.json'),label+' movable props',error,report=report)
            placed,unresolved=0,0
        return placed,unresolved
    # The props only read the prepared manifest and the DMO catalog, and write
    # their own folder: build them beside the collision archive and the map.
    # 'props' is then the time left waiting for them after write_map.
    with ThreadPoolExecutor(max_workers=1) as background:
        pending_props=background.submit(movable_props)
        collision=district_work/'collision.rwcmset'
        build_archive(manifest_path,collision)
        finished('collision_archive')
        write_map(manifest_path,final,collision,report,
                  prepared_spawn=tuple(start['position']) if start else spawn.result(label),
                  prepared_heading=start['heading'] if start else 0.)
        finished('write_map')
        placed,unresolved=pending_props.result()
    finished('props')
    report(f'{label}: placed {placed} authored DMO instances, {unresolved} unresolved templates')
    # Validation runs in the parent (_install, MapValidator) as each map finishes.
    entry={'name':label,'path':'maps/'+final.name,'sha256':digest(final)}
    if source:entry['source']=source
    remove_intermediate(district_work,work)
    finished('hash_and_cleanup')
    entry['phase_seconds']=timings
    return entry


# Groups whose outputs a refresh that does not rebuild them hard-links into the new installation (see `_install`).
LINKED_GROUPS=('audio',)


def shared_changed(shared):
    """Linked files whose size or modification time moved since they were linked, i.e. written in place."""
    def moved(path,before):
        try:now=os.stat(path)
        except OSError:return False  # replaced or removed: the previous installation's copy is untouched
        return now.st_ino==before.st_ino and (now.st_size,now.st_mtime_ns)!=(before.st_size,before.st_mtime_ns)
    return [path for path,before in shared if moved(path,before)]


def install(iso,base,game_exe,report,game_root=None,refresh=False,finalize=None):
    from .setup_state import setup_lock
    from .setup_budget import lowered_priority
    # Setup's own work (extraction, the character customiser) also runs below
    # normal priority; the previous priority returns afterwards.
    with setup_lock(base), lowered_priority():
        return _install(iso,base,game_exe,report,game_root,refresh,finalize)


def _install(iso,base,game_exe,report,game_root=None,refresh=False,finalize=None):
    # The core budget follows how busy the machine is when setup starts (setup_budget.measure_load); both entry
    # points (install() and customiser_setup.install) come through here.
    from .setup_budget import measure_load,MAPS_DONE
    measured='SKATE_SETUP_MEASURED_CORES' in os.environ
    measure_load(report)
    try:
        return _install_stages(iso,base,game_exe,report,game_root,refresh,finalize)
    finally:
        os.environ.pop(MAPS_DONE,None)
        if not measured:os.environ.pop('SKATE_SETUP_MEASURED_CORES',None)


def _install_stages(iso,base,game_exe,report,game_root=None,refresh=False,finalize=None):
    from .versions import fingerprints, changed_groups, installed, GROUPS
    from .group_receipts import damaged, record
    from .setup_state import atomic_json
    from . import asset_exports as exports
    target_versions=fingerprints()
    previous=installed(base) if refresh else None
    # The player's DLC packages (optional, research b374 / b410): their identity joins the hud group's, so adding or
    # removing a pack refreshes the menus; with no packages the versions stay exactly as before.
    from . import dlc as dlc_packages
    dlc_folder=dlc_packages.folder(iso if iso is not None else game_root, previous[1].get('dlc') if previous else None,
                                   Path(base).resolve().parent)
    dlc_identity=dlc_packages.signature(dlc_folder)
    # What the folder held (every file, packages or not): the game's start-up check compares it with the folder now.
    dlc_files=dlc_packages.listing(dlc_folder)
    if dlc_identity:
        target_versions['hud']=target_versions['hud']+'+dlc:'+dlc_identity
        report(f'DLC packages found in {dlc_folder}')
    # Only packs that carry map streams change the maps group (b415): a menu-only pack never reconverts the maps.
    dlc_maps_identity=dlc_packages.signature(dlc_folder,dlc_packages.has_maps) if dlc_identity else ''
    if dlc_maps_identity:
        # The map packs also carry their worlds' skies and audio (b414 / b417), and their AI skater lines, census and
        # roads (the living world; Sanatorium, Danny Way, Maloof and Art Gallery have lines).
        for group in ('maps','environment','audio','livingworld'):
            target_versions[group]=target_versions[group]+'+dlc:'+dlc_maps_identity
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
    if previous:groups.update(damaged(*previous, exclude=groups, report=report))
    if game_root is not None and (game_root/'.partial-source').is_file() and groups:
        # A refresh from an ISO started with only the checked disc files (iso_refresh.py): get the whole disc first.
        from .iso_refresh import NeedsFullSource
        raise NeedsFullSource(', '.join(sorted(groups)))
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
                    'outputs':outputs(previous[0]), 'source':source, 'dlc':str(dlc_folder) if dlc_identity else None,
                    'dlc_files':dlc_files})
        remove_setup_logs(previous[0],report)
        remove_stale_installations(base,previous[0],report)
        report('Game assets are current')
        return previous[0]
    install_id=uuid.uuid4().hex
    stage=base/'installations'/install_id
    stage.mkdir(parents=True)
    shared=[]  # (file, stat when linked): hard links shared with the previous installation
    if previous:
        report('Preparing the update…')
        immutable_maps = ({(previous[0]/item['path']).resolve() for item in
                           json.loads((previous[0]/'maps.json').read_text())}
                          if 'maps' not in groups else set())
        from .customiser_cache import SOURCES as character_stages
        sets=previous[0]/'assets/private/customisation/sets'
        immutable_sets=[p.resolve() for p in sets.glob('*') if p.is_dir()
                        and all((p/(name+'-complete.json')).is_file() for name in character_stages)]
        # The audio's outputs (4.9 GB, written only by the audio step) share storage too when the audio group is not
        # rebuilt (just verified by damaged()). Not every group: other steps write into core's stock/dlc and the living
        # world's hand props in place (the maps step), which reached the previous installation (laptop 2026-10-23).
        from .group_receipts import files as group_files
        unchanged={p.resolve() for g in LINKED_GROUPS if g not in groups for p in group_files(previous[0],g)}
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
            def link_rebuilt_map(src,dst):
                # A maps rebuild replaces each map file (the writers unlink first), so the previous files can be
                # shared instead of copied (1.4 GB); shared_changed() refuses to publish if one was written in place.
                try:
                    os.link(src,dst)
                    shared.append((Path(dst),os.stat(dst)))
                except OSError:shutil.copy2(src,dst)
                return dst
            def copy_asset(src,dst):
                resolved=Path(src).resolve()
                if resolved in unchanged:
                    try:
                        os.link(src,dst)
                        shared.append((Path(dst),os.stat(dst)))
                    except OSError:shutil.copy2(src,dst)
                elif any(resolved.is_relative_to(root) for root in immutable_sets):
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
                    copy_function=(copy_map if 'maps' not in groups else link_rebuilt_map) if entry.name=='maps'
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
        # The HUD and menu movies (their own process) read only the disc and the stock collections and write only their
        # own folders: they run beside the stages below and are joined before validation (doc 08 "Export speed pass").
        # In-process stages stay one at a time (NumPy's BLAS crashed when two threads used it at once).
        hud_job=None
        if 'hud' in groups:
            hud_job=Background(lambda:exports.hud(game_root,stage,work,report,log,dlc=dlc_folder if dlc_identity else None),'hud')
        if 'character' in groups:
            exports.character(game_root,stage,work,report,log,converted)
        # The player's DLC map packs: their map jobs and collection vaults, read once for the groups that use them.
        dlc_jobs,dlc_vaults=[],[]
        if dlc_maps_identity and groups&{'environment','maps','audio'}:
            from .optional_content import CONTENT_ERRORS as DLC_ERRORS
            try:
                dlc_jobs,dlc_vaults=dlc_packages.load(dlc_folder,game_root,work)
            except DLC_ERRORS as error:
                report(f'DLC maps unavailable ({error})')
        # The map jobs (their own processes) read only the disc, the map starts and the movable-object catalog and write
        # only their maps, props and status files: they start here and run beside the HUD, environment, audio and
        # living world stages; each map is validated further down, once the validator runs (doc 08 "Export speed pass").
        map_run=None
        workers=map_workers() if 'maps' in groups else 0
        if 'maps' in groups:
            from .optional_content import CONTENT_ERRORS, note
            report('Preparing authored movable-object models')
            from .dynamic_props import prepare_catalog
            catalog_ok=True
            try:
                prepare_catalog(game_root,work/'dmo')
                (private/'native-props/props-availability.json').unlink(missing_ok=True)
            except CONTENT_ERRORS as error:
                catalog_ok=False
                if (work/'dmo').exists():remove_intermediate(work/'dmo',work)
                note(private/'native-props/props-availability.json','Movable props',error,report=report)
            # Map jobs: every disc worldDIST_<D>.big, then each district stream in the player's DLC packs (b415).
            jobs=[{'archive':a,'district':None,'label':a.stem.removeprefix('worldDIST_'),'source':None}
                  for a in (game_root/'data/content').glob('worldDIST_*.big')]
            jobs+=[j for j in dlc_jobs if not any(j['label'].lower()==k['label'].lower() for k in jobs)]
            jobs.sort(key=lambda j:(j['label']!='University',j['label'].lower()))
            from .map_starts import prepare as map_starts
            # The DLC packs' collection rows and locators (b412): their maps' authored starts, and for the engine
            # (map lists, map audio) beside the disc's collections in private/stock/dlc.
            if dlc_vaults:
                rows=private/'stock/dlc';rows.mkdir(parents=True,exist_ok=True)
                for v in dlc_vaults:
                    (rows/(v['vault']+'-collections.json')).write_text(json.dumps(v['converted']),encoding='utf-8')
            try:
                starts=map_starts(game_root,converted,dlc_vaults)
            except CONTENT_ERRORS as error:
                starts={}
                report(f'Authored map starts unavailable ({error}); using geometric spawns')
            (work/'map-starts.json').write_text(json.dumps(starts),encoding='utf-8')
            report(f'Converting {len(jobs)} maps with {workers} workers')
            def map_job(job):
                result=work/(job['label']+'.json')
                named=[] if job['source'] is None else ['--district',job['district'],'--label',job['label'],'--source',job['source']]
                with (stage/(job['label']+'-conversion.log')).open('w',encoding='utf-8') as map_log:
                    run(task(TOOLS/'asset_pipeline/map_job.py','--archive',job['archive'],'--stage',stage,
                             '--game-exe',game_exe,'--result',result,*named),map_log,report)
                return json.loads(result.read_text(encoding='utf-8'))
            pool=ThreadPoolExecutor(max_workers=workers)
            # Start the expensive districts together so one does not
            # remain queued behind a string of small parks.
            futures={pool.submit(map_job,j):j for j in sorted(jobs,key=lambda j:-j['archive'].stat().st_size)}
            map_run=(pool,futures)
        # Customiser workers take the map jobs' cores once those end (setup_budget.customiser_limit).
        from .setup_budget import MAPS_DONE
        maps_done=work/'maps-done';os.environ[MAPS_DONE]=str(maps_done)
        if map_run is None:maps_done.touch()
        customiser=None  # Background customiser beside the other stages
        customiser_process=False  # it runs as its own process (not an in-process thread)
        post,post_result=None,{}  # Background post-map exports
        try:
            # As its own process (stage_job.py) the customiser needs nothing from the stages below (it reads only stock,
            # default-skater and disc data and writes only assets/private/customisation): it starts here, with one more
            # memory slot than beside the maps alone (doc 08 "Export speed pass").
            # customiser_setup.install's finalize is customiser_setup.prepare(game_root, stage/'assets'); not edited
            # there, since every customis*.py is in the customiser's fingerprint (gotchas "FINGERPRINT TRAP").
            if (finalize and getattr(finalize,'__module__','').endswith('customiser_setup')
                    and overlap_customiser(workers+1)):
                report('Preparing the character customiser alongside the other stages')
                customiser=Background(lambda:run_stage('customiser',game_root,stage,work,log,report),'customiser')
                customiser_process=True
            if 'environment' in groups:
                exports.environment(game_root,stage,work,report,log,converted,game_exe,dlc_vaults)
                # The unlock lines of the player's DLC packages (b458: retail's second unlock list; the Challenge Map shows
                # a hidden-flag location only when it is unlocked). No packages = an empty list (a fresh save's view).
                from .optional_content import CONTENT_ERRORS as UNLOCK_ERRORS
                target=stage/'assets/private/dlc-unlocks.json'
                try:
                    lines=dlc_packages.unlocks(dlc_folder) if dlc_identity else []
                    target.parent.mkdir(parents=True,exist_ok=True)
                    target.write_text(json.dumps({'version':1,'unlocks':lines},indent=1),encoding='utf-8')
                except UNLOCK_ERRORS as error:
                    report(f'DLC unlocks unavailable ({error})')
            # The living world reads only the disc and writes only private/living_world (its own process, so its NumPy work
            # runs beside the audio's); the props below read its tables, so it is joined first.
            living_job=None
            if 'livingworld' in groups:
                living_job=Background(lambda:run_stage('livingworld',game_root,stage,work,log,report,dlc_folder if dlc_maps_identity else None),'livingworld')
            if 'audio' in groups:
                exports.audio(game_root,stage,work,report,log,base/'tools',dlc_jobs,dlc_vaults,game_exe)
            if living_job is not None:living_job.join()
            # The hand props live in private/living_world (the livingworld group's folder) but read the maps' DMO
            # catalog: a livingworld-only refresh prepares the catalog itself, or it would publish without them.
            hand_props_catalog='maps' in groups and catalog_ok
            if 'livingworld' in groups and 'maps' not in groups:
                from .optional_content import CONTENT_ERRORS, note
                from .dynamic_props import prepare_catalog
                try:
                    prepare_catalog(game_root,work/'dmo')
                    hand_props_catalog=True
                except CONTENT_ERRORS as error:
                    report(f'Hand props skipped: movable-object catalog unavailable ({error})')
            if hand_props_catalog:
                # Ped hand props (DMO_Global templates) need the livingworld group's tables.json with dmo_models.
                try:
                    tables=private/'living_world/tables.json'
                    if tables.is_file() and 'dmo_models' in json.loads(tables.read_text(encoding='utf-8')).get('classes',{}):
                        from .hand_props import export as write_hand_props
                        props=write_hand_props(work/'dmo/catalog.json',private,report)['props']
                        report(f"Hand props: {sum('file' in p for p in props.values())} of {len(props)} exported")
                    else:
                        report('Hand props skipped: rerun the livingworld group (tables.json has no dmo_models)')
                except CONTENT_ERRORS as error:
                    # The map jobs read work/dmo meanwhile: it stays (they place their props as before).
                    note(private/'native-props/props-availability.json','Movable props',error,report=report)
            if hud_job is not None:hud_job.join()
            report('Validating skater, input and animation data')
            validator=start_validator(game_exe,stage/'assets',log,report)
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
                # The customiser reads only stock, default-skater and disc data (all
                # prepared above) and writes only assets/private/customisation, so it
                # runs beside the map jobs instead of after them.
                if finalize and customiser is None and overlap_customiser(workers):
                    report('Preparing the character customiser alongside the maps')
                    customiser=Background(lambda:finalize(stage),'customiser')
                def post_maps():
                    """Map names, teleporters, visual indicators and park layouts: they read only the disc, the collections
                    and the packs (whose map BIGs are already copied whole) and write only their own files."""
                    # Each map's Skate 3 name for the lists (map_names.py; user 2026-10-20: retail names for every map).
                    try:
                        from .map_names import names as map_names,disc_language
                        text=disc_language(game_root,work/'map-names')
                        for archive in sorted({j['archive'] for j in dlc_jobs}):
                            for label,value in dlc_packages.language(BigArchive(archive)).items():text.setdefault(label,value)
                        named=map_names(converted,dlc_vaults,text)
                    except CONTENT_ERRORS as error:
                        named={}
                        report(f'Map names unavailable ({error}); the lists show file names')
                    # Teleporters (teleporters.py, research b420 / b421): the disc's and the packs' teleport challenges per map.
                    bigs=[]
                    try:
                        from .teleporters import export as write_teleporters
                        bigs=dlc_packages.extract_bigs(dlc_folder,work/'dlc',lambda paths:any(p.lower().startswith(('data/db/challenge','data/db/progressionbanks/','data/content/missions/')) for p in paths))
                        write_teleporters(game_root,converted,dlc_vaults,bigs,work/'teleporters',stage/'assets/private/teleporters.json',text if named else None,report)
                        # A success clears an older failure's note (it was copied forward and reported again at every refresh).
                        (private/'map-status'/'teleporters-availability.json').unlink(missing_ok=True)
                    except CONTENT_ERRORS as error:
                        (stage/'assets/private/teleporters.json').unlink(missing_ok=True)
                        note(private/'map-status'/'teleporters-availability.json','Teleporters',error,report=report)
                    # Challenges (challenges.py, doc 42): own the spot volumes, start and owned / killed it points per challenge.
                    try:
                        from .challenges import export as write_challenges
                        write_challenges(game_root,dlc_vaults,bigs,work/'challenges',stage/'assets/private/challenges.json',text if named else None,report)
                        (private/'map-status'/'challenges-availability.json').unlink(missing_ok=True)
                    except CONTENT_ERRORS as error:
                        (stage/'assets/private/challenges.json').unlink(missing_ok=True)
                        note(private/'map-status'/'challenges-availability.json','Challenges',error,report=report)
                    # Career progression (progression.py, research b887): groups, actions, rewards, challenge rewards.
                    try:
                        from .progression import export as write_progression
                        write_progression(game_root,bigs,work/'progression',stage/'assets/private/progression.json',report)
                        (private/'map-status'/'progression-availability.json').unlink(missing_ok=True)
                    except CONTENT_ERRORS as error:
                        (stage/'assets/private/progression.json').unlink(missing_ok=True)
                        note(private/'map-status'/'progression-availability.json','Progression',error,report=report)
                    # Challenge visual indicators (visual_indicators.py, research b532 / b533): the teleporters' floating icons.
                    try:
                        from .visual_indicators import export as write_indicators
                        from .dlc import merged
                        write_indicators(game_root,merged(converted,list(dlc_vaults)),stage/'assets/private/visual-indicators.json',report)
                        (private/'map-status'/'visual-indicators-availability.json').unlink(missing_ok=True)
                    except CONTENT_ERRORS as error:
                        (stage/'assets/private/visual-indicators.json').unlink(missing_ok=True)
                        note(private/'map-status'/'visual-indicators-availability.json','Visual indicators',error,report=report)
                    # skate.Park layouts (park_layout.py, research b424 / b425 / b431): the pieces each park world places on
                    # its stream, from the disc and the packs (a pack's recipe, models and textures after the disc's).
                    try:
                        from .park_layout import Sources,export as write_park_layouts,world_layouts,teleporter_roles
                        from .dlc import merged
                        from .environment import Collections
                        park_bigs=dlc_packages.extract_bigs(dlc_folder,work/'dlc',lambda paths:any(p.lower().startswith(('data/skatepark/','data/content/recipe/','data/content/static/')) for p in paths)) if dlc_jobs else []
                        sources=Sources([BigArchive(game_root/'data/big/miscload.big'),BigArchive(game_root/'data/big/miscboot.big'),BigArchive(game_root/'data/content/parkassets.big'),*map(BigArchive,park_bigs)])
                        shutil.rmtree(stage/'assets/private/park-layouts',ignore_errors=True)
                        park_rows=Collections(merged(converted,list(dlc_vaults)))
                        write_park_layouts(sources,world_layouts(park_rows),stage/'assets/private/park-layouts',report,teleporter_roles(park_rows))
                        (private/'map-status'/'park-layouts-availability.json').unlink(missing_ok=True)
                    except CONTENT_ERRORS as error:
                        shutil.rmtree(stage/'assets/private/park-layouts',ignore_errors=True)
                        note(private/'map-status'/'park-layouts-availability.json','skate.Park layouts',error,report=report)
                    return named
                # Beside the map wait below when no in-process customiser thread runs (park layouts use NumPy; two
                # threads in NumPy crashed, gotchas 2026-10-24); joined before the map catalog (doc 08 "Export speed pass").
                if customiser is None or customiser_process:
                    post=Background(lambda:post_result.update(named=post_maps()),'post-maps')
                completed={}
                pool,futures=map_run
                with pool:
                    for future in as_completed(futures):
                        label=futures[future]['label']
                        try:
                            completed[label]=future.result()
                            # Validate while the remaining maps keep converting.
                            checked=validate(stage/completed[label]['path'])
                        except CONTENT_ERRORS as error:
                            completed.pop(label,None)
                            (maps/(label+'.skate')).unlink(missing_ok=True)
                            props=private/'native-props'/(label+'.skate')
                            if props.is_dir():remove_intermediate(props,private)
                            note(private/'map-status'/(label+'-availability.json'),label,error,report=report)
                            continue
                        (private/'map-status'/(completed[label]['name']+'-availability.json')).unlink(missing_ok=True)
                        record_validation(private,completed[label],checked,report)
                        report(f"Converted {len(completed)}/{len(jobs)} maps: {completed[label]['name']}")
                maps_done.touch()
        except BaseException:
            if map_run is not None:
                # Stop the queued map jobs and let the running ones end before the stage is cleaned up.
                map_run[0].shutdown(wait=True,cancel_futures=True)
            # A customiser process writes into the stage too: let it end first (its own error is secondary here).
            if customiser is not None:customiser.thread.join()
            if post is not None:post.thread.join()
            raise
        if post is not None:post.join()
        if 'maps' in groups:
            catalog=[completed[j['label']] for j in jobs if j['label'] in completed]
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
                    target=stage/old['path'];target.parent.mkdir(parents=True,exist_ok=True)
                    # Still the linked previous file when its job did not run (a kept DLC map): nothing to copy.
                    if not (target.is_file() and os.path.samefile(src,target)):shutil.copy2(src,target)
                    catalog.append(old)
                    note(private/'map-status'/(old['name']+'-availability.json'),old['name'],
                         RuntimeError('New source map unavailable; previous map passed validation'),retained=True,report=report)
            # Exclude invalid old retail maps from the runtime directory scan.
            valid_paths={item['path'] for item in catalog}
            if previous:
                for old in json.loads((previous[0]/'maps.json').read_text()):
                    if old['path'] not in valid_paths:(stage/old['path']).unlink(missing_ok=True)
            if not catalog:raise RuntimeError('No playable map could be prepared or recovered. Restore at least one worldDIST_*.big archive beside default.xex and retry; the previous installation has been kept.')
            named=post_result['named'] if post is not None else post_maps()
            for item in catalog:
                if item['name'] in named:item.update(display=named[item['name']]['name'],name_id=named[item['name']]['name_id'])
        # DLC maps kept without their folder keep their teleporters, layouts, rows, unlocks and audio (USER-ASKS 259).
        if previous and not dlc_identity:
            from .retained import carry,kept_dlc_maps
            disc=[a.stem.removeprefix('worldDIST_') for a in (game_root/'data/content').glob('worldDIST_*.big')]
            kept=kept_dlc_maps(json.loads((previous[0]/'maps.json').read_text()),disc,stage/'maps')
            carry(previous[0]/'assets/private',private,kept,report)
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
    changed=shared_changed(shared)
    if changed:
        listed='\n'.join(f'  {path}' for path in changed)
        raise RuntimeError(f'Setup changed {len(changed)} file(s) shared with the previous installation; the update was '
                           f'not published:\n{listed}')
    # Publish after core validation and all optional outcomes have been recorded.
    marker=base/'installation.json.new'
    marker.write_text(json.dumps({'version':1,'directory':'installations/'+install_id,'source':source,'source_hash':source_hash,'pipelines':target_versions,'outputs':outputs(stage),
                                  'dlc':str(dlc_folder) if dlc_identity else None,'dlc_files':dlc_files}),encoding='utf-8')
    marker.replace(base/'installation.json')
    remove_setup_logs(stage,report)
    remove_stale_installations(base,stage,report)
    report(f'Setup complete ({len(warnings)} warnings or unavailable/retained components; see setup-report.json)' if warnings else 'Setup complete')
    return stage
