"""Extract retail sky geometry and textures through RX2 GUID bindings."""
import json,struct,sys
from pathlib import Path
from tools.owned_game.big import BigArchive

def convert(game_root, assets):
    sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'vendor/utt'))
    import mdl_parser,rx2_parser
    archive=BigArchive(game_root/'data/big/miscload.big')
    entries={e.path:e for e in archive.entries}
    output=assets/'private/native-skies';output.mkdir(parents=True,exist_ok=True)
    for name,suffix in [('University',''),('DownTown','_downtown'),('Industrial','_industrial')]:
        prefix='data/content/world/models/DIST_skybox'+suffix
        raw=archive.read(entries[prefix+'.rx2'])
        model=mdl_parser.parse_rx2(raw)
        table=rx2_parser.RX2File(raw);table.parse()
        diffuse=None
        for section in table.entries:
            if section.type_id!=0x00eb0005:continue
            o=section.f0;h=struct.unpack_from('>8I',raw,o)
            if (h[4]-h[3])//h[1]!=32:raise ValueError('Unexpected sky material layout')
            for i in range(h[1]):
                v=struct.unpack_from('>8I',raw,o+h[3]+i*32)
                at=o+v[0];kind=raw[at:raw.index(b'\0',at)]
                if kind==b'diffuse':diffuse=(v[4]<<32)|v[5]
        textures=rx2_parser.parse_rx2(archive.read(entries[prefix+'_Textures.rx2']))
        handle=None
        for section in textures.entries:
            if section.type_id!=0x00eb000b:continue
            o=section.f0;n,start=struct.unpack_from('>2I',textures.data,o)
            for i in range(n):
                _,_,hi,lo,kind,index=struct.unpack_from('>6I',textures.data,o+start+i*24)
                if (hi<<32)|lo==diffuse:handle=index
        texture=next((t for t in textures.textures if t.index==handle),None)
        if texture is None:raise ValueError('Sky diffuse GUID did not resolve: '+name)
        if len(model.meshes)!=1 or model.meshes[0].uvs is None:raise ValueError('Unexpected sky mesh layout')
        mesh=model.meshes[0]
        (output/(name+'.rgba')).write_bytes(texture.rgba)
        (output/(name+'.json')).write_text(json.dumps({
            'width':texture.width,'height':texture.height,
            'positions':mesh.vertices.tolist(),'uvs':mesh.uvs.tolist(),
            'indices':mesh.faces.reshape(-1).tolist(),
        },separators=(',',':')),encoding='utf-8')
