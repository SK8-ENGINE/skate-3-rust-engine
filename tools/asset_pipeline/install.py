"""Local owned-disc installation. No game content is downloaded or packaged."""
from pathlib import Path
import hashlib,json,os,shutil,subprocess,sys,urllib.request,uuid,zipfile
from tools.owned_game.big import BigArchive
from .vlt import convert as convert_vlt
from .physics_skeleton import convert as convert_skeleton

TOOLS=Path(__file__).resolve().parents[1]
XISO_URL='https://github.com/XboxDev/extract-xiso/releases/download/build-202505152050/extract-xiso-Win64_Release.zip'
XISO_SHA='fec88d03c7efd6205ab09be4abba70c0afd0eb27a5709f0a6235b828ba5ac11e'

def digest(path):
    with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()

def remove_intermediate(path,root):
    target=path.resolve();root=root.resolve()
    if target==root or not target.is_relative_to(root):
        raise RuntimeError('Refusing to remove a path outside conversion workspace')
    shutil.rmtree(target)

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

def run(args,log,report):
    kwargs={'creationflags':subprocess.CREATE_NO_WINDOW} if os.name=='nt' else {}
    external=os.name=='nt' and getattr(sys,'frozen',False) and Path(args[0]).resolve()!=Path(sys.executable).resolve()
    if external:
        # External tools and the game must load their own libraries, not
        # the setup bundle's DLL directory inherited by child processes.
        import ctypes
        ctypes.windll.kernel32.SetDllDirectoryW(None)
        env=os.environ.copy()
        bundle=Path(sys._MEIPASS).resolve()
        env['PATH']=os.pathsep.join(p for p in env.get('PATH','').split(os.pathsep)
                                   if p and not Path(p).resolve().is_relative_to(bundle))
        kwargs['env']=env
    try:
        child=subprocess.Popen([str(a) for a in args],stdout=subprocess.PIPE,stderr=subprocess.STDOUT,
                               text=True,encoding='utf-8',errors='replace',**kwargs)
    finally:
        if external:ctypes.windll.kernel32.SetDllDirectoryW(sys._MEIPASS)
    with child as process:
        for line in process.stdout:
            log.write(line);log.flush()
        if process.wait():raise RuntimeError('Conversion failed. See '+str(log.name))

def task(script,*args):
    if getattr(sys,'frozen',False):return [sys.executable,'--task',str(script),*map(str,args)]
    return [sys.executable,str(script),*map(str,args)]

def extract(archive,destination,entries=None):
    data=BigArchive(archive)
    data.extract_entries(data.entries if entries is None else [e for e in data.entries if entries(e)],destination)
    return data

def convert_map(archive,work,maps,stage,game_exe,log,report):
    map_tools=TOOLS/'vendor/university/tools/vanilla_map_extraction/tools'
    sys.path.insert(0,str(map_tools))
    from prepare_hawaiian_dream import prepare
    from prepare_university import EXCLUDED_NORMAL_TEXTURE_IDS
    from build_retail_collision_archive import build_archive
    from .map_writer import write as write_map
    district=archive.stem.removeprefix('world')
    label=district.removeprefix('DIST_')
    district_work=work/district
    extract(archive,district_work/'raw')
    stream=district_work/'raw/data/content/world/stream'/district
    if not stream.is_dir():raise RuntimeError('Missing district stream '+str(stream))
    manifest_path=prepare(stream_directory=stream,output_root=district_work/'intermediate',
        utt_root=TOOLS/'vendor/utt',district_name=district,map_name=label,
        package_name='Skate 3 owned disc',cache_format='skate3-rust-map-v1',
        texture_stream_names=('Tex',),excluded_normal_texture_ids=EXCLUDED_NORMAL_TEXTURE_IDS)
    collision=district_work/'collision.rwcmset'
    build_archive(manifest_path,collision)
    final=maps/(label+'.skate')
    write_map(manifest_path,final,collision,report)
    report('Checking converted map: '+label)
    run([game_exe,'--assets',stage/'assets','--map',final,'--check-assets'],log,report)
    entry={'name':label,'path':'maps/'+final.name,'sha256':digest(final)}
    remove_intermediate(district_work,work)
    return entry


def install(iso,base,game_exe,report,game_root=None):
    base=base.resolve();base.mkdir(parents=True,exist_ok=True)
    lock=base/'setup.lock'
    try:fd=os.open(lock,os.O_CREAT|os.O_EXCL|os.O_WRONLY)
    except FileExistsError:raise RuntimeError('Setup is already running, or a previous setup was interrupted. Close it before retrying; an abandoned setup.lock can be removed from '+str(base))
    os.close(fd)
    try:
        install_id=uuid.uuid4().hex
        stage=base/'installations'/install_id
        stage.mkdir(parents=True)
        private=stage/'assets/private';private.mkdir(parents=True)
        maps=stage/'maps';maps.mkdir()
        work=stage/'conversion';work.mkdir()
        with (stage/'setup.log').open('w',encoding='utf-8') as log:
            if game_root is None:
                iso=iso.resolve()
                if not iso.is_file() or iso.suffix.lower()!='.iso':raise RuntimeError('Select an Xbox 360 Skate 3 ISO')
                extractor=dependency(base/'tools','extract-xiso',XISO_URL,XISO_SHA,report)
                game_root=work/'disc'
                report('Extracting your ISO')
                run([extractor,'-x',iso,'-d',game_root],log,report)
            else:game_root=game_root.resolve()
            for required in ['default.xex','data/big/miscload.big','data/big/miscboot.big','data/big/db.big',
                             'data/content/createacharacter.big','data/content/worldDIST_University.big']:
                if not (game_root/required).is_file():raise RuntimeError('This is not a supported Skate 3 disc: missing '+required)
            report('Extracting animation banks, graphs and gameplay inputs')
            stock=private/'stock'
            extract(game_root/'data/big/miscload.big',stock)
            extract(game_root/'data/big/miscboot.big',stock,lambda e:e.path.lower()=='data/config/input.cfg')
            # Some disc banks are loose files rather than members of miscload.
            loose=game_root/'data/anim'
            if loose.is_dir():shutil.copytree(loose,stock/'data/anim',dirs_exist_ok=True)
            report('Converting physics and difficulty settings')
            database=work/'database'
            extract(game_root/'data/big/db.big',database,lambda e:Path(e.path).name.lower() in {
                'skaterschema.bin','skaterschema.vlt','skatercollections.bin','skatercollections.vlt'})
            names=(TOOLS/'asset_pipeline/names.txt').read_text(encoding='utf-8').splitlines()
            converted=convert_vlt(database/'data/db/skaterschema',database/'data/db/skatercollections',names)
            (stock/'skater-collections.json').write_text(json.dumps(converted),encoding='utf-8')
            skeleton=convert_skeleton(stock/'data/anim/OnBoard.abin')
            (stock/'physics-skeletons.json').write_text(json.dumps(skeleton),encoding='utf-8')
            report('Preparing the skater model and textures')
            manifest=json.loads((TOOLS/'default_skater_retail_manifest.json').read_text())
            needed=set()
            for c in manifest['components']:
                needed.add(f"data/content/createacharacter/model/cas_db/{c['slot']}/0x{c['model_id']}.rx2".lower())
                needed.update(f'data/content/createacharacter/texture/0x{x}.rx2'.lower() for x in c['textures'].values())
            extract(game_root/'data/content/createacharacter.big',stock,lambda e:e.path.lower() in needed)
            character=work/'character'
            run(task(TOOLS/'extract_default_skater.py','--owned-data-root',stock,'--work-root',character,
                     '--private-root',private/'default_skater','--utt-root',TOOLS/'vendor/utt'),log,report)
            report('Building the skater model and rig')
            from .character_glb import convert as write_character
            write_character(character/'selected/models',private,manifest)
            game_manifest={'version':1,'character_scene':'private/skater.glb','initial_animation':'R_IDLE_HCOM_000',
                           'action_graph':'private/stock/data/state/ActionGraph_OnBoard.stategraph',
                           'motion_graph':'private/stock/data/state/MotionGraph_OnBoard.stategraph'}
            (private/'game.json').write_text(json.dumps(game_manifest),encoding='utf-8')
            report('Validating skater, input and animation data')
            run([game_exe,'--assets',stage/'assets','--test-world','--check-assets'],log,report)
            archives=list((game_root/'data/content').glob('worldDIST_*.big'))
            archives.sort(key=lambda p:(p.stem!='worldDIST_University',p.name.lower()))
            catalog=[]
            for number,archive in enumerate(archives,1):
                report(f'Converting map {number}/{len(archives)}: {archive.stem}')
                catalog.append(convert_map(archive,work,maps,stage,game_exe,log,report))
            if not any(m['name']=='University' for m in catalog):raise RuntimeError('University was not converted')
            report('Validating installed runtime inputs')
            run([game_exe,'--assets',stage/'assets','--test-world','--check-assets'],log,report)
            settings=stage/'settings';settings.mkdir()
            (settings/'default-map.json').write_text(json.dumps('maps/University.skate'),encoding='utf-8')
            (stage/'maps.json').write_text(json.dumps(catalog,indent=2),encoding='utf-8')
            remove_intermediate(work,stage)
            # Publish only after every conversion and the runtime's own load succeeds.
            marker=base/'installation.json.new'
            marker.write_text(json.dumps({'version':1,'directory':'installations/'+install_id}),encoding='utf-8')
            marker.replace(base/'installation.json')
            report('Setup complete')
            return stage
    finally:lock.unlink(missing_ok=True)
