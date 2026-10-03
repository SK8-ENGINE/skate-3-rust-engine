"""Publish owned retail Marquee models without autorigging or retargeting."""
import argparse, hashlib, json, os, shutil, struct, sys, tempfile, time
import xml.etree.ElementTree as ET
from pathlib import Path
import numpy as np
from PIL import Image, ImageChops
from tools.owned_game.big import BigArchive
from tools.extract_default_skater import import_rx2_parser, decode_texture
from tools.asset_pipeline.character_glb import Glb, convert
from tools.asset_pipeline.retail_character import RX2, decode_dense_morphs
from tools.asset_pipeline.marquee_assets import Resources
from tools.asset_pipeline.optional_content import CONTENT_ERRORS

# GetCACSettings 82590BE0..82590DBC, TU3: six named styles, Aggressive otherwise.
STYLES={'danny_way':'DannyWay','mike_carroll':'MikeCarroll','pj_ladd':'PJLadd',
        'jason_dill':'JasonDill','jerry_hsu':'JerryHsu','rob_dyrdek':'RobDyrdek'}
LABELS={'isaak':'Isaac Clarke (Dead Space)','steak':'Meat Man','dem_bones':'Dem Bones',
        'dr_pepper':'Dr Pepper Mascot','deerman':'Deerman of Dark Woods','pj_ladd':'PJ Ladd',
        'reda':'Giovanni Reda','attiba_jefferson':'Atiba Jefferson','cuz':'Cuz Parry'}

def finalize_glb(path):
    """Remove numerical perspective residue; retain native affine bind transforms."""
    data=path.read_bytes();length=struct.unpack_from('<I',data,12)[0]
    writer=Glb();writer.doc=json.loads(data[20:20+length]);writer.data=bytearray(data[28+length:])
    for node in writer.doc['nodes']:
        if 'matrix' in node:
            m=node['matrix']
            if max(abs(m[i]) for i in [3,7,11])+abs(m[15]-1)>1e-6:raise ValueError('Non-affine retail joint')
            for i in [3,7,11]:m[i]=0.
            m[15]=1.
    for skin in writer.doc['skins']:
        a=writer.doc['accessors'][skin['inverseBindMatrices']];v=writer.doc['bufferViews'][a['bufferView']]
        for j in range(a['count']):
            offset=v.get('byteOffset',0)+a.get('byteOffset',0)+j*64
            for i in [3,7,11]:struct.pack_into('<f',writer.data,offset+i*4,0.)
            struct.pack_into('<f',writer.data,offset+60,1.)
    for mesh in writer.doc['meshes']:
        for p in mesh['primitives']:
            for a in p['attributes'].values():writer.doc['bufferViews'][writer.doc['accessors'][a]['bufferView']]['target']=34962
            writer.doc['bufferViews'][writer.doc['accessors'][p['indices']]['bufferView']]['target']=34963
    temporary=path.with_suffix('.tmp');writer.save(temporary);os.replace(temporary,path)

def roster(rows):
    rows={r['key']:r for r in rows if r['class']=='characters_marquee'}
    result=[]
    for key,row in rows.items():
        chain=[]; node=row
        while node:
            if node['key'] in chain:raise ValueError('Cyclic character inheritance')
            chain.append(node['key']);node=rows.get(node['parent'])
        if not {'pro_skaters','ip'}.intersection(chain):continue
        recipe=row['fields'].get('Hash_6C9F05D8DBE7A492',{}).get('data','')
        if not recipe:continue
        result.append({'key':key,'recipe':recipe,'name':LABELS.get(key,key.replace('_',' ').title()),
                       'category':'Special' if 'ip' in chain else 'Pro',
                       'animation_style':STYLES.get(key,'Aggressive')})
    return sorted(result,key=lambda r:r['name'])

def _publish(dest,write):
    """Write via a unique temporary file and rename, so parallel roster workers
    sharing work/ caches never see a partial file (identical content either way).

    The first publisher wins and an existing dest is never replaced: on Windows,
    renaming over a file another worker is opening fails for one side (WinError 5
    for the writer, Errno 13 for the reader). os.rename does not overwrite there,
    and every worker writes identical bytes, so an existing dest is kept."""
    temporary=dest.with_name(f'{dest.stem}.{os.getpid()}.tmp{dest.suffix}')
    try:
        write(temporary)
        for attempt in range(20):
            if dest.exists():return
            try:
                (os.rename if os.name=='nt' else os.replace)(temporary,dest);return
            except FileExistsError:
                return
            except PermissionError:
                time.sleep(.05*(attempt+1))  # transient lock (e.g. antivirus scan)
        os.replace(temporary,dest)
    finally:
        temporary.unlink(missing_ok=True)

def prepare(game,assets,library,collections,work,only=None,report_path=None):
    archive=BigArchive(game/'data/content/marquee.big')
    resources=Resources(archive)
    work.mkdir(parents=True,exist_ok=True)
    parser=import_rx2_parser(Path(__file__).resolve().parents[1]/'vendor/utt')
    def extract(path):
        dest=work/'source'/path
        if not dest.exists():
            dest.parent.mkdir(parents=True,exist_ok=True)
            _publish(dest,lambda tmp:tmp.write_bytes(resources.read(path)))
        return dest
    def texture(tid):
        dest=work/'decoded'/(tid+'.png');dest.parent.mkdir(exist_ok=True)
        if not dest.exists():
            source=extract('data/content/marquee/texture/0x'+tid+'.rx2')
            _publish(dest,lambda tmp:decode_texture(parser,source,tmp))
        return dest
    records=roster(json.loads(collections.read_text())['collections']);report=[]
    for item in records:
        if only and item['key'] not in only:continue
        try:
            resources.recipe(item['recipe'])
        except CONTENT_ERRORS as error:
            report.append({**item,'status':'unavailable','error':str(error)})
            print(f'Unavailable optional character {item["name"]}: {error}',flush=True)
            continue
        key=item['key'];recipe_name=item['recipe'];folder=work/key;models=folder/'models';mats=folder/'materials'
        models.mkdir(parents=True,exist_ok=True);mats.mkdir(exist_ok=True)
        identity=hashlib.sha256(('native-marquee-v1:'+key).encode()).hexdigest();target=library/'entries'/identity
        try:
            xml=extract('data/content/recipe/marquee/'+recipe_name+'.xml');root=ET.fromstring(xml.read_bytes())
            material_defs={m.attrib['id']:m for m in root.findall('mat')}
            recipe={'components':[],'morph_assembly':{'expected_targets':{},'face_targets':[]},'preset':{'body_mods':{}}}
            for component in root.findall('comp'):
                slot=component.attrib['n'];mods=component.findall('mod')
                if len(mods)!=1:raise ValueError('Ambiguous native component '+slot)
                lod=next(l for l in mods[0].findall('lod') if l.get('idx')=='0')
                raw=extract(f"data/content/marquee/model/{recipe_name}/{slot}/{lod.attrib['arenaid']}.rx2")
                dest=models/slot/raw.name;dest.parent.mkdir(exist_ok=True);shutil.copyfile(raw,dest)
                mid=lod.find('matinst/matvar').attrib['id'];mat=material_defs[mid]
                tex={s.attrib['chn']:s.attrib['id'].removeprefix('0x') for s in mat.findall('sp')}
                diffuse=Image.open(texture(tex['diffuse'])).convert('RGBA')
                if 'alpha' in tex:
                    alpha=Image.open(texture(tex['alpha'])).convert('RGB').getchannel('R');diffuse.putalpha(alpha)
                diffuse.save(mats/(slot+'_base_color.png'))
                if 'normal' in tex:
                    a=np.asarray(Image.open(texture(tex['normal'])).convert('RGBA'),dtype=float)
                    x=a[:,:,3]/127.5-1.;y=a[:,:,1]/127.5-1.;s=np.maximum(np.sqrt(x*x+y*y),1.);x/=s;y/=s
                    z=np.sqrt(np.maximum(0.,1.-x*x-y*y));Image.fromarray(np.rint((np.stack((x,y,z),2)*.5+.5)*255).astype('uint8')).save(mats/(slot+'_normal.png'))
                if 'specular' in tex:ImageChops.invert(Image.open(texture(tex['specular'])).convert('L')).save(mats/(slot+'_roughness.png'))
                parsed=RX2.parse_rx2(str(dest));mesh=next(m for m in parsed['meshes'] if m.get('positions') and m.get('indices'))
                morphs=decode_dense_morphs(dest,parsed,len(mesh['positions']),RX2)
                recipe['morph_assembly']['expected_targets'][slot]=[m['name'] for m in morphs]
                recipe['components'].append({'slot':slot,'tint':[1,1,1],'textures':tex,'alpha_mode':'MASK' if 'alpha' in tex else 'OPAQUE'})
            target.parent.mkdir(parents=True,exist_ok=True)
            if not target.exists():
                with tempfile.TemporaryDirectory(prefix='.native-',dir=library) as tmp:
                    stage=Path(tmp)/'entry';stage.mkdir()
                    convert(models,assets/'private',recipe,output=stage/'character.glb',materials=mats)
                    finalize_glb(stage/'character.glb')
                    sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'mixamo_to_skate'))
                    from library_import import thumbnail
                    thumbnail(stage/'character.glb',stage/'preview.png')
                    metadata={'version':1,'id':identity,'name':item['name'],'native':item,
                              'source_recipe_sha256':hashlib.sha256(xml.read_bytes()).hexdigest()}
                    (stage/'manifest.json').write_text(json.dumps(metadata,indent=2))
                    (stage/'recipe.json').write_text(json.dumps(recipe,indent=2))
                    stage.rename(target)
            report.append({**item,'id':identity,'status':'ready'})
            print('READY',key,flush=True)
        except CONTENT_ERRORS as e:
            report.append({**item,'status':'error','error':str(e)});print('ERROR',key,str(e),flush=True)
    (report_path or work/'report.json').write_text(json.dumps(report,indent=2))
    return report

def prepare_parallel(game,assets,library,collections,work,workers):
    """prepare() split over worker processes (customisation_workers.py --roster).

    Characters write disjoint outputs (library/entries/<id>, work/<key>); shared
    work caches are written atomically. Reports are merged back into roster
    order, so the result equals a serial prepare(). Falls back to prepare()
    for one worker or if any worker fails."""
    records=roster(json.loads(collections.read_text())['collections'])
    keys=[item['key'] for item in records]
    # An empty `only` means "all" to prepare(): never hand a worker no keys.
    workers=min(workers,len(keys))
    if workers<2:return prepare(game,assets,library,collections,work)
    from tools.asset_pipeline.customisation_workers import run_parallel
    work.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='roster-workers-',dir=work) as tmp:
        requests=[];reports=[]
        for index in range(workers):
            request=Path(tmp)/f'{index}.json';result=Path(tmp)/f'{index}-report.json'
            request.write_text(json.dumps(dict(game=str(game),assets=str(assets),library=str(library),
                collections=str(collections),work=str(work),only=keys[index::workers],report=str(result))))
            requests.append(['--roster',request]);reports.append(result)
        failures=[error for error in run_parallel(requests,workers) if error is not None]
        if failures or not all(r.is_file() for r in reports):
            return prepare(game,assets,library,collections,work)  # finishes whatever is missing
        merged={item['key']:item for r in reports for item in json.loads(r.read_text())}
    report=[merged[key] for key in keys]
    (work/'report.json').write_text(json.dumps(report,indent=2))
    return report

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--game',type=Path,required=True);p.add_argument('--assets',type=Path,required=True)
    p.add_argument('--library',type=Path,required=True);p.add_argument('--collections',type=Path,required=True);p.add_argument('--work',type=Path,required=True);p.add_argument('--only',nargs='*')
    a=p.parse_args();r=prepare(a.game,a.assets,a.library,a.collections,a.work,a.only)
    raise SystemExit(1 if any(x['status']=='error' for x in r) else 0)
