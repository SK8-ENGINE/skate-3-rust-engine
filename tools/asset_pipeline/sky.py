"""Extract authored retail skies through world references and RX2 GUID bindings."""
import json
import struct
import sys
from pathlib import Path
from tools.owned_game.big import BigArchive
from .environment import Collections, NAMES, field, key_hash, sky_parameters, world_environment, fog_parameters


def _texture(textures, guid):
    handle = None
    for section in textures.entries:
        if section.type_id != 0x00eb000b:
            continue
        o = section.f0
        n, start = struct.unpack_from('>2I', textures.data, o)
        for i in range(n):
            _, _, hi, lo, _, index = struct.unpack_from('>6I', textures.data, o+start+i*24)
            if (hi<<32)|lo == guid:
                handle = index
    texture = next((t for t in textures.textures if t.index == handle), None)
    if texture is None:
        raise ValueError(f'Sky texture GUID did not resolve: {guid:016X}')
    return texture


# Material changes retail makes to a sky model by its name (sky_overrides.json; mods may add rows).
OVERRIDES = Path(__file__).resolve().parent / 'sky_overrides.json'


def _bind(raw, model, prep, model_name):
    """Per mesh: its material group (the GUID binding backdrop.py uses) and metadata, with the override rows applied
    the way retail's copy differs (b427: the material's shader replaced, listed parameters removed)."""
    groups, bindings = prep._bind_material_groups_by_guid(
        raw, prep._group_material_parameters(model.materials), len(model.meshes), allow_import_order_fallback=False)
    rows = json.loads(OVERRIDES.read_text(encoding='utf-8')).get('models', {})
    changes = next((v['materials'] for k, v in rows.items() if k.casefold() == model_name.casefold()), {})
    for i, binding in enumerate(bindings):
        change = changes.get(str(binding['group_index']))
        if change:
            group = {k: v for k, v in groups[i].items() if k not in change.get('remove', [])}
            group['AttribulatorMaterialName'] = [change['shader']]
            groups[i] = group
    return groups, bindings, [prep._material_metadata(groups, i) for i in range(len(model.meshes))]


def _sky(collections, row, read, mdl_parser, rx2_parser, prep):
    """A world's sky: its environment, dome texture, sun gradient and dome mesh, plus the model's other meshes
    (Hawaiian Dream's trees and rocks, b427 / b428) for the backdrop (raises when it does not resolve)."""
    env = world_environment(collections, row['key'])
    raw = read(row, 'data/content/'+env['sky_model']+'.rx2')
    model = mdl_parser.parse_rx2(raw)
    table = rx2_parser.RX2File(raw)
    table.parse()
    groups, bindings, materials = _bind(raw, model, prep, env['sky_model'])
    # The dome: the one mesh with a `sky.` material (every disc sky has only that mesh).
    domes = [i for i, m in enumerate(materials) if (m['shader_name'] or '').startswith('sky.')]
    if len(domes) != 1 or model.meshes[domes[0]].uvs is None:
        raise ValueError('Unexpected sky mesh layout')
    dome = domes[0]
    from .backdrop import texture_groups
    channels = texture_groups(raw, table, ('diffuse', 'specular'))[bindings[dome]['group_index']]
    textures_raw = read(row, 'data/content/'+env['sky_textures']+'.rx2')
    textures = rx2_parser.parse_rx2(textures_raw)
    diffuse = _texture(textures, channels['diffuse'])
    sun = _texture(textures, channels['specular'])
    env.update(sky_parameters(collections, materials[dome]['shader_name']))
    env['fog_frame'] = fog_parameters(env['fog'])
    mesh = model.meshes[dome]
    if len(sun.rgba) != sun.width*sun.height*4 or sun.height != 16:
        raise ValueError('Unexpected sky sun-gradient dimensions')
    scenery = dict(raw=raw, model=model, groups=groups, bindings=bindings, textures=textures, table=table,
                   selected=[(i, m) for i, m in enumerate(materials) if i != dome], model_name=env['sky_model'],
                   texture_name=env['sky_textures'])
    return env, diffuse, sun, mesh, scenery


def convert(game_root, assets, converted, dlc_vaults=()):
    """Every disc world with a worldDIST_<D>.big, plus the worlds of the player's DLC map packs (`dlc_vaults`,
    asset_pipeline.dlc.load): a DLC sky is read from its own pack first, then from miscload.big (b417)."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/utt'))
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/university/tools/vanilla_map_extraction/tools'))
    import mdl_parser
    import rx2_parser
    import prepare_hawaiian_dream as prep
    from .backdrop import texture_groups, write_model
    from .dlc import merged, stream_districts
    collections = Collections(merged(converted, list(dlc_vaults)))
    archive = BigArchive(game_root/'data/big/miscload.big')
    entries = {e.path: e for e in archive.entries}
    packs = {}
    for v in dlc_vaults:
        if v['vault'] not in packs:
            pack = BigArchive(v['big'])
            packs[v['vault']] = (pack, {e.path.replace('\\', '/'): e for e in pack.entries})
    def read(row, path):
        pack = packs.get(row.get('vault'))
        if pack and path in pack[1]:
            return pack[0].read(pack[1][path])
        return archive.read(entries[path])
    output = assets/'private/native-skies'
    output.mkdir(parents=True, exist_ok=True)
    available = {p.stem.removeprefix('world') for p in (game_root/'data/content').glob('worldDIST_*.big')}
    for pack, paths in packs.values():
        available |= set(stream_districts(paths))
    count = 0
    skipped = []
    for (cls, _), row in collections.rows.items():
        if cls != key_hash('world'):
            continue
        own_stream = next((v['data'] for k, v in row['fields'].items()
                           if key_hash(k) == key_hash('WorldStream')), None)
        if own_stream not in available:
            continue
        # The map's label (asset_pipeline.dlc.map_jobs: the stream folder without DIST_ / Dist_).
        name = own_stream[5:] if own_stream.lower().startswith('dist_') else own_stream
        try:
            sky = _sky(collections, row, read, mdl_parser, rx2_parser, prep)
        except (KeyError, ValueError) as error:
            if not row.get('vault'):
                raise
            skipped.append(f"{row['key']} ({error!r})")
            continue
        env, diffuse, sun, mesh, scenery = sky
        # A multi-mesh sky's other meshes (retail draws them with the world's environment effects, b428) become the
        # map's render-only backdrop, unless the world has its own presentation model (backdrop.py).
        fields, _ = collections.resolve('world', row['key'])
        if scenery['selected'] and key_hash('Hash_951898F6C0FA6856') not in fields:
            backdrops = assets/'private/native-backdrops'
            backdrops.mkdir(parents=True, exist_ok=True)
            channel_groups = texture_groups(scenery['raw'], scenery['table'], prep.RETAIL_TEXTURE_CHANNELS)
            write_model(scenery['raw'], scenery['model'], scenery['groups'], scenery['bindings'], scenery['selected'],
                        scenery['textures'], channel_groups, backdrops/(name+'.skate'), own_stream,
                        scenery['model_name'], scenery['texture_name'])
        (output/(name+'.rgba')).write_bytes(diffuse.rgba)
        (output/(name+'.sun.rgba')).write_bytes(sun.rgba)
        (output/(name+'.json')).write_text(json.dumps({
            'width': diffuse.width, 'height': diffuse.height,
            'positions': mesh.vertices.tolist(), 'uvs': mesh.uvs.tolist(),
            'indices': mesh.faces.reshape(-1).tolist(),
            'environment': env, 'sun_width': sun.width, 'sun_height': sun.height,
            'source_sha256': row['sha256'],
        }, separators=(',', ':')), encoding='utf-8')
        count += 1
    if not count:
        raise ValueError('No authored world skies resolved')
    if skipped:
        print('DLC worlds without a resolvable sky: ' + ', '.join(skipped), flush=True)
    return count


if __name__ == '__main__':
    import argparse
    import tempfile
    from .vlt import convert as convert_vlt
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-root', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    args = parser.parse_args()
    # A targeted update reads only the small database pair and sky resources.
    with tempfile.TemporaryDirectory(prefix='skate-sky-') as work:
        database = BigArchive(args.game_root/'data/big/db.big')
        needed = {'skaterschema.bin', 'skaterschema.vlt', 'skatercollections.bin', 'skatercollections.vlt'}
        database.extract_entries([e for e in database.entries if Path(e.path).name.lower() in needed], Path(work))
        stem = Path(work)/'data/db'
        names = (Path(__file__).parent/'names.txt').read_text(encoding='utf-8').splitlines()
        converted = convert_vlt(stem/'skaterschema', stem/'skatercollections', [*names, *NAMES])
        print('Prepared', convert(args.game_root, args.assets, converted), 'authored skies')
