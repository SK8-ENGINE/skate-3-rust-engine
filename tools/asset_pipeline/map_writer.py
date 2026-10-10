"""Direct retail cache -> SKATE14 writer. All geometry remains in Y-up metres."""
from pathlib import Path
import io,json,struct,sys,zlib
from collections import deque
from concurrent.futures import ThreadPoolExecutor
import numpy as np
from PIL import Image
from tools.asset_pipeline.retail_material import _retail_shader_family,_retail_render_flags

def u(f,*v):f.write(struct.pack('<'+'I'*len(v),*v))
def floats(f,*v):f.write(struct.pack('<'+'f'*len(v),*v))
def string(f,s):
    b=str(s).encode();u(f,len(b));f.write(b)
def packed_blob(data):
    """The bytes `stored` writes for `data`: header and payload."""
    packed=zlib.compress(data,1)
    method=1
    if len(packed)>=len(data):method=0;packed=data
    return struct.pack('<II',method,len(packed)),packed
def stored(f,data):
    for part in packed_blob(data):f.write(part)

def packed_texture(root,name,entry):
    if 'rgba' in entry:
        width,height=entry['width'],entry['height']
        pixels=np.frombuffer((root/entry['rgba']).read_bytes(),dtype=np.uint8).reshape(height,width,4)
        rgba=(pixels if entry.get('cube_faces')==6 else pixels[::-1]).tobytes()
    else:
        with Image.open(root/entry['png']) as source:
            image=source.convert('RGBA')
            if entry.get('cube_faces')!=6:image=image.transpose(Image.Transpose.FLIP_TOP_BOTTOM)
            width,height=image.size;rgba=image.tobytes()
    result=io.BytesIO();string(result,name);u(result,width,height,1);stored(result,rgba)
    return result.getvalue()

def write_textures(output,root,textures,workers=None):
    # zlib releases the GIL. Bound outstanding work to one texture per worker
    # rather than retaining an entire district's decoded/compressed images;
    # results are written in name order whatever the worker count.
    if workers is None:
        from tools.asset_pipeline.setup_budget import job_threads
        workers=job_threads()
    workers=max(1,workers)
    names=iter(sorted(textures))
    with ThreadPoolExecutor(max_workers=workers) as pool:
        pending=deque()
        for _ in range(workers):
            name=next(names,None)
            if name is not None:pending.append(pool.submit(packed_texture,root,name,textures[name]))
        while pending:
            output.write(pending.popleft().result())
            name=next(names,None)
            if name is not None:pending.append(pool.submit(packed_texture,root,name,textures[name]))
def normalise(a):return a/np.maximum(np.linalg.norm(a,axis=1,keepdims=True),1e-20)

GROUND_HEADROOM=2.5  # clear metres above a spawn surface
GROUND_RADIUS=25.    # neighbourhood searched for lower ground
GROUND_STEP=1.       # a surface this far above nearby ground is a roof/platform
GROUND_CANDIDATES=512
# Only compact districts (skate parks) use ground_spawn. City districts are
# kilometres wide with collision under the streets, where "lowest surface"
# is underground; they keep the original rule.
GROUND_MAX_EXTENT=500.

def ground_spawn(triangles):
    """Walkable ground nearest the area-weighted centre of flat surfaces.

    Retail collision winding is not consistent (some park floors face down),
    so orientation is ignored. A candidate must be the lowest surface at its
    XZ, have headroom, and have no ground more than GROUND_STEP lower nearby;
    that rejects roofs and floating panels at a park's edge. Returns None if
    no candidate qualifies."""
    t=np.asarray(triangles,dtype=np.float64)
    a,b,c=t[:,0],t[:,1],t[:,2]
    ab=b-a;ac=c-a
    cross=np.cross(ab,ac);length=np.linalg.norm(cross,axis=1)
    flat=(length>=4)&(np.abs(cross[:,1])>=.9*length)
    if not flat.any():return None
    points=((a+b+c)/3)[flat];weights=length[flat]
    middle=(points[:,[0,2]]*weights[:,None]).sum(0)/weights.sum()
    order=np.argsort(((points[:,[0,2]]-middle)**2).sum(1))
    det=ab[:,0]*ac[:,2]-ac[:,0]*ab[:,2]
    low=np.minimum(np.minimum(a,b),c);high=np.maximum(np.maximum(a,b),c)
    flat_low=low[flat];flat_high=high[flat]
    for index in order[:GROUND_CANDIDATES]:
        x,y,z=points[index]
        # Extents, not centres: one large floor triangle can reach the candidate.
        near=((flat_low[:,0]<=x+GROUND_RADIUS)&(flat_high[:,0]>=x-GROUND_RADIUS)
              &(flat_low[:,2]<=z+GROUND_RADIUS)&(flat_high[:,2]>=z-GROUND_RADIUS))
        if flat_low[near,1].min()<y-GROUND_STEP:continue
        under=((low[:,0]<=x)&(x<=high[:,0])&(low[:,2]<=z)&(z<=high[:,2])&(np.abs(det)>=1e-12)).nonzero()[0]
        d=det[under];dx=x-a[under,0];dz=z-a[under,2]
        first=(dx*ac[under,2]-ac[under,0]*dz)/d;second=(ab[under,0]*dz-dx*ab[under,2])/d
        inside=(first>=-1e-5)&(second>=-1e-5)&(first+second<=1.00001)
        heights=(a[under,1]+first*ab[under,1]+second*ac[under,1])[inside]
        if heights.size and heights.min()<y-.05:continue
        above=heights[heights>y+.05]
        if above.size and above.min()-y<GROUND_HEADROOM:continue
        return (float(x),float(y)+1.,float(z))
    return None

class SpawnSelector:
    """University keeps its authored XZ. Compact districts prefer ground_spawn;
    large ones, and parks where it finds nothing, use the original
    nearest-origin upward triangle."""
    def __init__(self, district_name):
        self.university=district_name=='DIST_University'
        self.best=None
        self.triangles=[]

    def consider(self, meshes):
        university=self.university;best=self.best
        for mesh in meshes:
            if not mesh.triangles:continue
            if not university:
                self.triangles.append(np.asarray([(tri.a,tri.b,tri.c) for tri in mesh.triangles],dtype=np.float32))
            if university and not (mesh.bounds_min[0]<=330<=mesh.bounds_max[0] and mesh.bounds_min[2]<=-710<=mesh.bounds_max[2]):continue
            if not university and best is not None:
                nearest=sum(max(mesh.bounds_min[j],-mesh.bounds_max[j],0.)**2 for j in (0,2))
                if nearest>best[0]+.01:continue
            triangles=np.asarray([(tri.a,tri.b,tri.c) for tri in mesh.triangles],dtype=np.float64)
            a,b,c=triangles[:,0],triangles[:,1],triangles[:,2]
            cross=np.cross(b-a,c-a);length=np.linalg.norm(cross,axis=1)
            valid=(length>=4)&(cross[:,1]>=.9*length)
            a,b,c=a[valid],b[valid],c[valid]
            if not len(a):continue
            if university:
                ab=b-a;ac=c-a;dx=330.-a[:,0];dz=-710.-a[:,2]
                det=ab[:,0]*ac[:,2]-ac[:,0]*ab[:,2]
                valid=np.abs(det)>=1e-12
                first=np.divide(dx*ac[:,2]-ac[:,0]*dz,det,out=np.zeros_like(det),where=valid)
                second=np.divide(ab[:,0]*dz-dx*ab[:,2],det,out=np.zeros_like(det),where=valid)
                valid&=(first>=-1e-5)&(second>=-1e-5)&(first+second<=1.00001)
                height=a[:,1]+first*ab[:,1]+second*ac[:,1]
                points=np.column_stack((np.full(len(a),330.),height+1.,np.full(len(a),-710.)))
                scores=np.where(valid,np.abs(height-132.),np.inf)
            else:
                points=(a+b+c)/3;scores=points[:,0]**2+points[:,2]**2
                points[:,1]+=1.
            index=np.argmin(scores);score=scores[index]
            if np.isfinite(score) and (best is None or score<best[0]):best=(score,tuple(points[index]))
        self.best=best

    def result(self, map_name):
        if self.triangles:
            triangles=np.concatenate(self.triangles)
            span=triangles.reshape(-1,3).max(0)-triangles.reshape(-1,3).min(0)
            ground=ground_spawn(triangles) if max(span[0],span[2])<=GROUND_MAX_EXTENT else None
            if ground is not None:return ground
        if self.best is None:raise ValueError('No supported spawn surface in '+map_name)
        return self.best[1]


def spawn_point(manifest,root):
    from retail_collision_mesh import decode_rx2_clustered_meshes
    selector=SpawnSelector(manifest['district_name'])
    for entry in manifest['simulation_assets']:
        if not entry.get('collision_meshes'):continue
        bounds=[mesh['bounds'] for mesh in entry['collision_meshes']]
        # Other districts need every mesh: ground_spawn compares all surfaces.
        if selector.university:
            if not any(b['minimum'][0]<=330<=b['maximum'][0] and
                       b['minimum'][2]<=-710<=b['maximum'][2] for b in bounds):continue
        selector.consider(decode_rx2_clustered_meshes((root/entry['rx2']).read_bytes()))
    return selector.result(manifest['map_name'])

def write(manifest_path,output,collision,report=lambda _:None, *, render_only=False, prepared_spawn=None, prepared_heading=0., extensions=None):
    root=manifest_path.parent;m=json.loads(manifest_path.read_text());textures=m['textures']
    ids={name:i+1 for i,name in enumerate(sorted(textures))}
    excluded=set(m['normal_texture_policy']['excluded_texture_ids'])
    mats=io.BytesIO();vertices=io.BytesIO();indices=io.BytesIO();nv=ni=nm=0
    # Per-model index ranges in the final buffer let callers reference shared
    # geometry (MOBJ instances point at these without duplicating triangles).
    ranges=[]
    dtype=np.dtype([('p','<f4',(3,)),('n','<f4',(3,)),('uv','<f4',(2,)),('lm','<f4',(2,)),
                    ('mat','<u4'),('decal','<f4',(2,)),('frame','i1',(4,))])
    for number,model in enumerate(m['models']):
        if number%100==0:report(f"Writing {m['map_name']}: model {number+1}/{len(m['models'])}")
        first_index=ni
        with np.load(root/model['npz'],allow_pickle=False) as archive:
            for mesh in model['meshes']:
                i=mesh['index']
                # NpzFile does not cache reads. Load this mesh once, including
                # attributes reused by normals, UVs and tangent conversion.
                arrays={key:archive[key] for key in archive.files if key.endswith('_'+str(i))}
                pos=arrays[f'vertices_{i}'];faces=arrays[f'faces_{i}'].astype('<u4')
                if not len(pos) or not len(faces):continue
                if faces.max()>=len(pos):raise ValueError('Map face is outside vertex array')
                shader=mesh.get('shader_name') or '';roles=mesh.get('retail_texture_ids',{})
                albedo=mesh.get('texture_id');light=roles.get('lightmap') if f'lightmap_uvs_{i}' in arrays and not shader.startswith(('water.','ocean.')) else None
                normal=roles.get('normal');normal=None if normal in excluded else normal
                def tex(name):
                    if not name:return 0
                    if name not in ids:raise ValueError('Missing retail map texture '+name)
                    return ids[name]
                string(mats,f"{model['asset_id']}_{i}");u(mats,1);floats(mats,.82,0.,*( (0.,0.,0.) if albedo=='0x0000119903e3870a' else (.8,.8,.8)),.68,0.)
                u(mats,tex(albedo),tex(light));floats(mats,.25 if light else 0.);u(mats,tex(normal),0,0,mesh.get('alpha_mode',0));floats(mats,mesh.get('alpha_cutoff',.5))
                # The established exporter derives layers from the authored
                # alpha mode for the generated MAT_<texture-id> names.
                depth=3 if mesh.get('alpha_mode',0)==2 else 1 if mesh.get('alpha_mode',0)==1 else 0
                u(mats,3,1,0,depth,int(bool(shader)))
                if shader:
                    mats.write(struct.pack('<QIi',int(mesh.get('retail_material_guid','0'),0),int(mesh.get('retail_material_handle','0'),0),mesh.get('retail_material_group_index',-1)))
                    string(mats,shader);u(mats,_retail_shader_family(shader),_retail_render_flags(shader,mesh.get('alpha_mode',0)),len(roles))
                    for role,name in sorted(roles.items()):
                        uv=1 if role in {'lightmap','chromaticity','alpha'} else 2 if role=='decal' else 0
                        clamp=int(role in {'lightmap','chromaticity'} or (role=='decal' and not shader.startswith('environment.decal_tileable')))
                        string(mats,role);u(mats,tex(name),uv,clamp,clamp)
                    params=mesh.get('retail_parameters',{});u(mats,len(params))
                    for name,values in params.items():
                        string(mats,name);u(mats,len(values))
                        for value in values:string(mats,value)
                    string(mats,json.dumps({'asset_id':model['asset_id'],'mesh_index':i,'source_offsets':mesh['source_offsets']}))
                record=np.zeros(len(pos),dtype=dtype);record['p']=pos;record['mat']=nm+1
                normals=arrays.get(f'retail_normals_{i}',arrays.get(f'normals_{i}'))
                if normals is None:
                    normals=np.zeros_like(pos);face_normals=np.cross(pos[faces[:,1]]-pos[faces[:,0]],pos[faces[:,2]]-pos[faces[:,0]])
                    for corner in range(3):np.add.at(normals,faces[:,corner],face_normals)
                normals=normalise(normals);record['n']=normals
                for target,key in [('uv',f'uvs_{i}'),('lm',f'lightmap_uvs_{i}'),('decal',f'decal_uvs_{i}')]:
                    uv=np.array(arrays.get(key,arrays.get(f'uvs_{i}',np.zeros((len(pos),2)))),copy=True)
                    if target=='lm' and key in arrays and shader!='ocean.default':uv=np.abs(uv)
                    uv[:,1]=1.-uv[:,1];record[target]=uv
                if f'retail_tangents_{i}' in arrays:
                    tangent=np.array(arrays[f'retail_tangents_{i}'],copy=True);tangent-=normals*np.sum(tangent*normals,axis=1,keepdims=True)
                    sign=np.array(arrays[f'retail_tangent_handedness_{i}'],copy=True).reshape(-1);sign[np.linalg.norm(tangent,axis=1)<1e-12]=0
                    binormal=np.cross(normals,normalise(tangent))*sign[:,None]
                    record['frame'][:,:3]=np.rint(np.clip(binormal,-1,1)*127).astype('i1');record['frame'][:,3]=np.rint(sign*127).astype('i1')
                if not np.isfinite(pos).all():raise ValueError('Non-finite map geometry')
                vertices.write(record.tobytes());indices.write((faces+nv).astype('<u4').tobytes());nv+=len(pos);ni+=faces.size;nm+=1
        ranges.append((first_index,ni-first_index))
    report('Selecting starting position: '+m['map_name']);spawn=(0.,0.,0.) if render_only else (prepared_spawn if prepared_spawn is not None else spawn_point(m,root))
    # Match the supplied exporter's environment defaults. Native sky shaders
    # remain a separate runtime feature; no geometry is synthesized here.
    environment=[.10,.36,.75,.64,.82,1.,.18,.24,.30,0.,11.,0.,18.,0.,
                 .045,.10,.26,1.,.32,.10,.05,.035,.06,.007,.015,.045,.045,.085,.17,.008,.014,.032,
                 1.,.96,.86,.42,.56,.92,1.,.18,.34,.10,1.,1.,1.]
    rails=m['grind_splines'];output.parent.mkdir(parents=True,exist_ok=True)
    # `extensions` is a list of (tag, schema, payload) triples, or a
    # callable invoked with the per-model index ranges once geometry is
    # final (MOBJ records reference those ranges).
    extra=[e if len(e)==3 else (e[0],1,e[1]) for e in (extensions(ranges) if callable(extensions) else extensions or [])]
    records=[(b'WMET',1,json.dumps(m,separators=(',',':')).encode())]+extra
    if not render_only:records.insert(0,(b'RWCM',1,collision.read_bytes()))
    from tools.asset_pipeline.setup_budget import job_threads
    threads=job_threads()
    # The geometry and extension blobs do not depend on the textures: compress
    # them on two more threads while the textures are written. Every blob is
    # written in the original order, so the file is byte-identical.
    with output.open('wb') as f, ThreadPoolExecutor(max_workers=2) as blobs:
        geometry=[blobs.submit(packed_blob,data) for data in (vertices.getvalue(),indices.getvalue(),b'')]
        packed_extensions=[(tag,schema,len(data),blobs.submit(packed_blob,data)) for tag,schema,data in records]
        del vertices,indices,records
        f.write(b'SKATE14\0');u(f,0x12345678);string(f,m['map_name']);floats(f,*spawn,0. if render_only else prepared_heading,*environment)
        u(f,nm,len(ids),nv,ni,0,len(rails),0,0,0);f.write(mats.getvalue())
        write_textures(f,root,textures,threads)
        for blob in geometry:
            for part in blob.result():f.write(part)
        for rail in rails:
            string(f,f"{rail['asset_id']}_{rail['section_index']}_{rail['rail_index']}")
            u(f,int(rail['closed']),1);f.write(struct.pack('<QQ',int(rail['spline_id'],0),int(rail['type_signature'],0)))
            u(f,rail['flags'],rail['trailing_word'],rail['segment_count'])
            for segment in rail['native_segment_payloads']:
                raw=bytes.fromhex(segment)
                if len(raw)!=120:raise ValueError('Invalid native spline segment')
                f.write(np.frombuffer(raw,dtype='>u4').astype('<u4').tobytes())
        u(f,len(packed_extensions))
        for tag,schema,size,blob in packed_extensions:
            f.write(tag);u(f,schema,size)
            for part in blob.result():f.write(part)
    report('Map written: '+m['map_name'])
    if not render_only:
        from tools.asset_pipeline.irradiance import write as write_irradiance
        write_irradiance(manifest_path, output.with_suffix('.irradiance'))

if __name__=='__main__':
    import argparse
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--collision',type=Path,required=True)
    args=parser.parse_args()
    sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'vendor/university/tools/vanilla_map_extraction/tools'))
    write(args.manifest,args.output,args.collision,lambda text:print(text,flush=True))
