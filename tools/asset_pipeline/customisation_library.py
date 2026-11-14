"""Prepare resident CAC geometry and shared textures outside the game loop."""
from pathlib import Path
import copy,json,shutil,sys,re,struct
import numpy as np
from PIL import Image,ImageChops
from tools.asset_pipeline.customisation_worker import ROOT,default_profile,flag
from tools.asset_pipeline.customisation_catalog import write_private
from tools.owned_game.big import BigArchive
from tools.asset_pipeline.optional_content import CONTENT_ERRORS

def friendly(name):
    text=name.lower().replace('_',' ')
    replacements=[('shortsleeve','short sleeve'),('longsleeve','long sleeve'),('tshirt','tee'),
        ('t-shirt','tee'),('buttonup','button-up'),('zipup','zip-up'),('flippedup','flipped up'),
        ('kneedown',''),('thighdown',''),('medhem',''),('lowhem',''),('highhem',''),
        ('sleeves pushed up','rolled sleeves'),('sleeves up','rolled sleeves'),('sleevup','rolled sleeves'),
        ('colourizable',''),('colorizable',''),('stampable',''),('logoable',''),('material',''),
        ('lambert1','original'),('newera','new era'),('45right','angled right'),('45left','angled left'),
        ('long sleeve tee','long-sleeve tee'),('short sleeve tee','tee'),
        ('short sleeve button-up shirt','short-sleeve shirt'),('long sleeve button-up shirt','long-sleeve shirt'),
        ('shirt ls button-up','long-sleeve shirt'),('top button only','top button'),
        ('long sleeve mens shirt vneck','long-sleeve v-neck'),('jeans straight leg baggy waistline','baggy straight jeans'),
        ('jeans straight leg baggy waistline','baggy straight jeans'),('sunglasses','shades'),('necklaces',''),('tattoo',''),('biglizard','big lizard'),
        ('blueflame','blue flame'),('radialsun','sunburst'),('tribalbody','tribal'),('pinstripe','pinstripe ')]
    for a,b in replacements:text=text.replace(a,b)
    text=re.sub(r'\b(male\d*|female\d*|unisex|mesh|mat|hmat|h|l|high|low|ropa|hem|inner)\b','',text)
    text=re.sub(r'\b(?:0x)?[a-f0-9]{8,}\b','',text)
    text=re.sub(r'([a-z])([0-9]+)\b',r'\1 \2',text)
    text=re.sub(r'\s+',' ',text).strip(' -')
    text=text.replace('jeans straight leg baggy waistline','baggy straight jeans').replace('long-sleeve tee rolled sleeves','rolled-sleeve tee')
    return text.title() or 'Original'


def variant_label(slot, model_name, native_name):
    label=friendly(native_name)
    worn=bool(re.search(r'(?:_| )d(?:_| |$)|dirty',native_name.lower()))
    label=re.sub(r'\b(Diffuse|Tga|D|Malemat|Skateboard|Featured Artist|Instance)\b','',label)
    label=re.sub(r'\b0\b','',label)
    if slot=='Feet': return 'Worn' if worn else 'New'
    model_words=set(friendly(model_name).lower().split())
    words=[w for w in label.split() if w.lower() not in model_words]
    label=' '.join(words).strip()
    for a,b in [('Ozzie Wright','Ozzie'),('Grass Roots Pullover Hoodie','Grass Roots'),
                ('Full Back New Era',''),('Hoodie Hooddown Pullover',''),('Hood Up',''),
                ('Hooddown',''),('Closed',''),('Open',''),('In4Mation Colab Zip','In4Mation'),
                ('Nightmare Catcher','Nightmare'),('Gonz Animal Bullfish','Bullfish')]: label=label.replace(a,b)
    label=re.sub(r'\s+',' ',label).strip()
    if not label: label='Original'
    return label+(' (Worn)' if worn else '')


def prune_unavailable(models, materials, errors):
    """Never expose a model selection that points at an absent material."""
    for key, model in list(models.items()):
        model['groups'] = [[mid for mid in group if mid in materials] for group in model['groups']]
        if not model['groups'] or any(not group for group in model['groups']):
            del models[key]
            errors.append(dict(model=key, error='No available material for a mesh group'))
        else:
            model['materials'] = model['groups'][0]


TEXTURE_PATH='data/content/createacharacter/texture/0x{}.rx2'

def requested_textures(catalog):
    """Texture ids prepare() decodes into decoded/, with its skips: no Misc
    models, LOD 0, each material once, 'diffuse' required. Error paths can make
    prepare() request fewer; prepare() removes warmed files it did not use."""
    seen=set();tids=[]
    for part in catalog['components']:
        if part['slot']=='Misc':continue
        for model in part['models']:
            lod=next((l for l in model['lods'] if l['index']==0),None)
            if lod is None:continue
            for v in (v for group in lod['material_instances'] for v in group):
                mat=catalog['materials'].get(v['id'])
                if v['id'] in seen or mat is None:continue
                tex={t['channel']:t['id'] for t in mat['textures']}
                if 'diffuse' not in tex:continue
                seen.add(v['id'])
                tids+=[tex[c] for c in ('diffuse','alpha','normal','specular') if c in tex]
    misc=next((p for p in catalog['components'] if p['slot']=='Misc'),None)
    for m in (misc or {}).get('models',[]):
        for v in (v for lod in m.get('lods',[]) for group in lod.get('material_instances',[]) for v in group):
            mat=catalog['materials'].get(v['id'])
            if mat and mat['flags'].get('cas.TattooCategory'):
                tex={t['channel']:t['id'] for t in mat['textures']}
                if 'decal' in tex:tids.append(tex['decal'])
    return list(dict.fromkeys(tids))

_texture_sources={}

def texture_sources(config):
    """The disc archive, its index and the RX2 parser, opened once per worker process."""
    key=(config['game_root'],config['directory'])
    if key not in _texture_sources:
        from tools.extract_default_skater import import_rx2_parser
        archive=BigArchive(Path(config['game_root'])/'data/content/createacharacter.big')
        _texture_sources[key]=(archive,{e.path.lower():e for e in archive.entries},import_rx2_parser(ROOT/'tools/vendor/utt'))
    return _texture_sources[key]

def warm_textures(config,tids):
    """Worker body (customisation_workers.py): decode textures into decoded/
    exactly as prepare()'s texture() does. Failures are left for prepare()."""
    from tools.extract_default_skater import decode_texture
    cache=Path(config['directory']);archive,entries,parser=texture_sources(config)
    decoded=cache/'decoded';decoded.mkdir(exist_ok=True)
    for tid in tids:
        src=decoded/(tid+'.png')
        if src.exists():continue
        path=TEXTURE_PATH.format(tid);dest=cache/'source'/path
        temporary=decoded/(tid+'.warm.png')
        try:
            if not dest.exists():write_private(dest,archive.read(entries[path.lower()]))
            decode_texture(parser,dest,temporary);temporary.replace(src)
        except CONTENT_ERRORS:
            temporary.unlink(missing_ok=True)

class ModelConverter:
    """Geometry-only GLB for one library model; shared by prepare() and the warm-up workers."""
    def __init__(self,config,archive=None,entries=None):
        from tools.asset_pipeline.retail_character import RX2,decode_dense_morphs,AnimSource,SkeletonSet
        from tools.asset_pipeline.character_glb import convert
        self.RX2,self.decode_dense_morphs,self.convert=RX2,decode_dense_morphs,convert
        self.assets=Path(config['assets']);self.cache=Path(config.get('directory',self.assets/'private/customisation'));self.out=self.cache/'library'
        self.archive=archive or BigArchive(Path(config['game_root'])/'data/content/createacharacter.big')
        self.entries=entries or {e.path.lower():e for e in self.archive.entries}
        self.recipe_base=base_recipe();self.reference=self.out/'reference'
        # Read-only shared rig inputs live only for this invocation.
        # Per-item meshes and morphs are discarded after conversion, bounding RAM.
        self.animation=AnimSource(self.assets/'private/stock/data/anim/OnBoard.abin')
        self.reference_skeleton=SkeletonSet(str(self.reference))
    def extract(self,path):
        dest=self.cache/'source'/path
        if not dest.exists():write_private(dest,self.archive.read(self.entries[path.lower()]))
        return dest
    def __call__(self,slot,key,lod,temporary_suffix='.tmp'):
        path=self.out/(key+'_v4.glb')
        if path.exists():return path
        source=self.extract(lod['path']);folder=self.out/'work'/key/'models'/slot;folder.mkdir(parents=True,exist_ok=True)
        mesh_file=folder/source.name
        if not mesh_file.exists():shutil.copyfile(source,mesh_file)
        parsed=self.RX2.parse_rx2(str(mesh_file));mesh=next(m for m in parsed['meshes'] if m.get('positions') and m.get('indices'))
        morphs=self.decode_dense_morphs(mesh_file,parsed,len(mesh['positions']),self.RX2)
        recipe=copy.deepcopy(self.recipe_base);recipe['components']=[dict(slot=slot,textures={},tint=[1.,1.,1.])]
        recipe['morph_assembly']['expected_targets']={slot:[m['name'] for m in morphs]}
        temporary=path.with_suffix(temporary_suffix)
        try:
            self.convert(folder.parent,self.assets/'private',recipe,output=temporary,geometry_only=True,live_morphs=True,reference_models=self.reference,
                         source=self.animation,reference_skeleton=self.reference_skeleton,
                         parsed_models={str(mesh_file):parsed},decoded_morphs={str(mesh_file):morphs})
        except BaseException:
            temporary.unlink(missing_ok=True);raise
        temporary.replace(path)
        return path

def base_recipe():
    recipe=json.loads((ROOT/'tools/default_skater_retail_manifest.json').read_text())
    recipe['morph_assembly']['live_targets']=['fat','thin']+recipe['morph_assembly']['face_targets']+['global_ethnicity_african','global_ethnicity_asian','global_ethnicity_caucasian']
    return recipe

def library_models(catalog):
    return [(part['slot'],model) for part in catalog['components'] if part['slot']!='Misc' for model in part['models']]

_library_models={}

def warm_models(config,keys,convert=None):
    """Worker body (customisation_workers.py): convert these library models.
    Failures are left for prepare(), which converts again and records the error."""
    directory=config['directory']
    if directory not in _library_models:  # parsed once per worker process, not per batch
        _library_models[directory]={m['id']:(slot,m) for slot,m in library_models(json.loads((Path(directory)/'catalog.json').read_text()))}
    models=_library_models[directory];convert=convert or ModelConverter(config)
    for key in keys:
        slot,model=models[key]
        try:convert(slot,key,next(l for l in model['lods'] if l['index']==0),'.warm.tmp')
        except CONTENT_ERRORS:pass

def warm_up(config,workers):
    """Decode the requested textures and convert the library models in parallel processes.

    Returns (warmed ids, ids whose source file the warm-up created) so that
    prepare() can remove exactly what a serial run would not have produced."""
    cache=Path(config['directory']);catalog=json.loads((cache/'catalog.json').read_text())
    tids=[t for t in requested_textures(catalog) if not (cache/'decoded'/(t+'.png')).exists()]
    keys=[m['id'] for _,m in library_models(catalog) if not (cache/'library'/(m['id']+'_v4.glb')).exists()]
    if workers<2:return set(),set()
    if len(tids)<2:tids=[]
    fresh={t for t in tids if not (cache/'source'/TEXTURE_PATH.format(t)).exists()}
    from tools.asset_pipeline.customisation_workers import batches,run_shared
    # Models first: they are the longer jobs, the textures fill in behind them.
    run_shared({'config':config},batches('models',keys,8)+batches('textures',tids,32),workers)  # a failed worker only leaves work for prepare()
    return set(tids),fresh

def derive(dest,kind,src,alpha_src=None):
    """Normal / rough map, or the diffuse with its alpha mask multiplied in ('combined')."""
    image=Image.open(src).convert('RGBA')
    if kind=='normal':
        a=np.array(image,dtype=np.float64)
        x=a[:,:,3]/127.5-1.;y=a[:,:,1]/127.5-1.;scale=np.maximum(np.sqrt(x*x+y*y),1.);x/=scale;y/=scale
        z=np.sqrt(np.maximum(0.,1.-x*x-y*y));image=Image.fromarray(np.rint((np.stack((x,y,z),axis=2)*.5+.5)*255.).astype('uint8'))
    elif kind=='rough':
        grey=ImageChops.invert(image.convert('L'));white=Image.new('L',image.size,255);image=Image.merge('RGB',(white,grey,white))
    elif kind=='combined':
        alpha=Image.open(alpha_src).convert('RGBA')
        alpha=alpha.resize(image.size,Image.Resampling.LANCZOS);mask=alpha.getchannel('A')
        if mask.getextrema()==(255,255):mask=alpha.convert('L')
        image.putalpha(ImageChops.multiply(image.getchannel('A'),mask))
    dest.parent.mkdir(exist_ok=True)
    temporary=dest.with_name(dest.stem+'.part.png')
    try:image.save(temporary);temporary.replace(dest)
    except BaseException:temporary.unlink(missing_ok=True);raise

def derived_jobs(catalog,out):
    """The derived textures prepare()'s material loop asks for, in its order (models without a GLB are skipped as there)."""
    seen=set();jobs={}
    for slot,model in library_models(catalog):
        lod=next((l for l in model['lods'] if l['index']==0),None)
        if lod is None or not (out/(model['id']+'_v4.glb')).exists():continue
        for v in (v for group in lod['material_instances'] for v in group):
            mid=v['id'];mat=catalog['materials'].get(mid)
            if mid in seen or mat is None:continue
            tex={t['channel']:t['id'] for t in mat['textures']}
            if 'diffuse' not in tex:continue
            seen.add(mid)
            if 'alpha' in tex and slot!='Hair':jobs.setdefault(tex['diffuse']+'_'+tex['alpha'],('combined',tex['diffuse'],tex['alpha']))
            if 'normal' in tex:jobs.setdefault(tex['normal']+'_normal',('normal',tex['normal']))
            if 'specular' in tex:jobs.setdefault(tex['specular']+'_rough',('rough',tex['specular']))
    return [(name,)+job for name,job in jobs.items()]

def derive_textures(config,jobs):
    """Worker body (customisation_workers.py). Inputs not decoded yet and failures are left for prepare()."""
    cache=Path(config['directory']);decoded=cache/'decoded';textures=cache/'library'/'textures'
    for name,kind,*tids in jobs:
        dest=textures/(name+'.png');sources=[decoded/(t+'.png') for t in tids]
        if dest.exists() or not all(s.exists() for s in sources):continue
        try:derive(dest,kind,*sources)
        except CONTENT_ERRORS:pass

def warm_derived(config,workers):
    """Derive the material textures in parallel processes after the warm-up.

    Returns the files it created; prepare() removes those its loop did not ask for."""
    cache=Path(config['directory']);textures=cache/'library'/'textures'
    jobs=[j for j in derived_jobs(json.loads((cache/'catalog.json').read_text()),cache/'library') if not (textures/(j[0]+'.png')).exists()]
    if workers<2 or len(jobs)<2:return set()
    from tools.asset_pipeline.customisation_workers import batches,run_shared
    run_shared({'config':config},batches('derive',jobs,32),workers)
    for leftover in textures.glob('*.part.png'):leftover.unlink()
    return {textures/(j[0]+'.png') for j in jobs if (textures/(j[0]+'.png')).exists()}

def prepare(config):
    from tools.extract_default_skater import import_rx2_parser,decode_texture,parse_fallback_recipe
    assets=Path(config['assets']);cache=Path(config.get('directory',assets/'private/customisation'));out=cache/'library';out.mkdir(exist_ok=True)
    catalog=json.loads((cache/'catalog.json').read_text());archive=BigArchive(Path(config['game_root'])/'data/content/createacharacter.big')
    entries={e.path.lower():e for e in archive.entries}
    def extract(path):
        dest=cache/'source'/path
        if not dest.exists():write_private(dest,archive.read(entries[path.lower()]))
        return dest
    recipe_base=base_recipe();reference=out/'reference'
    for component in recipe_base['components']:
        slot=component['slot'];model=next(m for p in catalog['components'] if p['slot']==slot for m in p['models'] if m['id']==component['asset_id'])
        src=extract(next(l for l in model['lods'] if l['index']==0)['path']);dest=reference/slot/src.name
        dest.parent.mkdir(parents=True,exist_ok=True)
        if not dest.exists():shutil.copyfile(src,dest)
    parser=import_rx2_parser(ROOT/'tools/vendor/utt');decoded=cache/'decoded';decoded.mkdir(exist_ok=True)
    warmed,fresh_sources=warm_up(config,config.get('workers',1));used=set()
    for leftover in out.glob('*.warm.tmp'):leftover.unlink()
    derived_made=warm_derived(config,config.get('workers',1));derived_used=set()
    def texture(tid,kind='raw'):
        used.add(tid)
        src=decoded/(tid+'.png')
        if not src.exists():decode_texture(parser,extract('data/content/createacharacter/texture/0x'+tid+'.rx2'),src)
        if kind=='raw':return src.relative_to(assets).as_posix()
        dest=out/'textures'/(tid+'_'+kind+'.png');derived_used.add(dest)
        if not dest.exists():derive(dest,kind,src)
        return dest.relative_to(assets).as_posix()
    model_data={};mat_data={};errors=[]
    defaults={x['material_id']:x for x in recipe_base['components']}
    convert_model=None
    for part in catalog['components']:
        for model in part['models']:
            if part['slot']=='Misc':continue  # Non-geometric stamp catalogue, indexed below.
            slot=part['slot'];key=model['id'];path=out/(key+'_v4.glb')
            # Only what the warm-up left (a failed worker, or no workers) is converted here.
            if convert_model is None and not path.exists():convert_model=ModelConverter(config,archive,entries)
            try:
                lod=next(l for l in model['lods'] if l['index']==0)
                if not path.exists():convert_model(slot,key,lod)
                groups=[[v['id'] for v in group] for group in lod['material_instances']]
                model_data[key]=dict(slot=slot,name=friendly(model['name']),flags={k[4:]:v for k,v in model['flags'].items() if v},
                    materials=groups[0],groups=groups,scene=path.relative_to(assets).as_posix())
                for v in (v for group in lod['material_instances'] for v in group):
                    try:
                        mid=v['id']
                        if mid in mat_data:continue
                        mat=catalog['materials'][mid];tex={t['channel']:t['id'] for t in mat['textures']}
                        if 'diffuse' not in tex:continue
                        diffuse=texture(tex['diffuse'])
                        if 'alpha' in tex and slot!='Hair':
                            alpha_path=assets/texture(tex['alpha']);combined=out/'textures'/(tex['diffuse']+'_'+tex['alpha']+'.png')
                            derived_used.add(combined)
                            if not combined.exists():derive(combined,'combined',assets/diffuse,alpha_path)
                            diffuse=combined.relative_to(assets).as_posix()
                        mat_data[mid]=dict(name=variant_label(slot,model['name'],v['name']),flags={k[4:]:val for k,val in mat['flags'].items() if val},
                            diffuse=diffuse,normal=texture(tex['normal'],'normal') if 'normal' in tex else None,
                            rough=texture(tex['specular'],'rough') if 'specular' in tex else None,
                            opacity=texture(tex['alpha']) if slot=='Hair' and 'alpha' in tex else None,
                            alpha='alpha' in tex,tint=defaults.get(mid,{}).get('tint',
                                [0.72,0.57,0.49] if mat['flags'].get('cas.SkinTone')=='light' else
                                [0.33,0.26,0.23] if mat['flags'].get('cas.SkinTone')=='dark' else
                                [0.02,0.01,0.01] if slot=='Hair' else [1.,1.,1.]),
                            metallic=.65 if slot=='SkateTruck' else 0.,roughness=.48 if slot in {'SkateTruck','SkateWheel'} else .72)
                    except CONTENT_ERRORS as error:
                        errors.append(dict(model=key,material=v.get('id'),slot=slot,error=str(error)))
            except CONTENT_ERRORS as e:errors.append(dict(model=key,slot=slot,error=str(e)))
        print('Prepared',part['slot'],len(model_data),'models',len(mat_data),'materials',flush=True)
    tattoos={}
    misc=next((p for p in catalog['components'] if p['slot']=='Misc'),None)
    variants=(v for m in (misc or {}).get('models',[]) for lod in m.get('lods',[])
              for group in lod.get('material_instances',[]) for v in group)
    for variant in variants:
        try:
            mid=variant['id'];mat=catalog['materials'][mid]
            if not mat['flags'].get('cas.TattooCategory'):continue
            tex={t['channel']:t['id'] for t in mat['textures']}
            tattoos[mid]=dict(name=friendly(variant['name']),texture=texture(tex['decal']),
                bounds=[float(v) for v in mat['flags']['cas.StampBorderConstraint'].split(',')])
        except CONTENT_ERRORS as error:
            errors.append(dict(tattoo=variant.get('id'),error=str(error)))
    # Warm-up must not add receipted files a serial run would not have made.
    for tid in warmed-used:
        (decoded/(tid+'.png')).unlink(missing_ok=True)
        if tid in fresh_sources:(cache/'source'/TEXTURE_PATH.format(tid)).unlink(missing_ok=True)
    for path in derived_made-derived_used:path.unlink(missing_ok=True)
    prune_unavailable(model_data,mat_data,errors)
    male=default_profile();male['gender']='male'
    female=copy.deepcopy(male);female['gender']='female';female['selections']={}
    raw=parse_fallback_recipe(assets/'private/stock/data/cacrecipes/SavedRecipeFallbackFemale.bin',16)
    for slot,items in raw['asset_lists'].items():
        if items:
            item=items[0];female['selections'][slot]={'asset_id':item['asset_id'],'material_id':item['models'][0]['material_id']}
    colours=[]
    native=json.loads((cache/'native.json').read_text())
    for row in native['collections']:
        if row['class']=='cac_colour' and row['key'].endswith('_clothing') and row['key']!='default_clothing':
            colours.append(dict(name=row['key'][:-9].replace('_',' ').title(),
                rgb=struct.unpack('>4f',bytes.fromhex(row['fields']['Hash_7827ED970A88B70C']['data']))[:3]))
    colours.sort(key=lambda c:c['name'])
    library=dict(version=3,colours=colours,tattoos=tattoos,models=model_data,materials=mat_data,defaults={'male':male,'female':female},
                 morphs=recipe_base['morph_assembly']['live_targets'],errors=errors)
    (cache/config.get('library_index','library.json')).write_text(json.dumps(library,separators=(',',':')))
    print('LIBRARY_READY',len(model_data),len(mat_data),'errors',errors,flush=True)
    return library

if __name__=='__main__':prepare(json.loads(Path(sys.argv[1]).read_text()))
