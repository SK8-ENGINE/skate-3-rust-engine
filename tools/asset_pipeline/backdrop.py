"""Export each district's global presentation model (sea, far shore, tree walls) outside district streams."""
import json
import hashlib
import re
import struct
import sys
import tempfile
from pathlib import Path

from .environment import Collections, field, key_hash
from .map_writer import write
from .sky import _texture
from tools.owned_game.big import BigArchive


def texture_groups(raw, table, channels):
    """Read binary channel GUIDs; display-name suffixes are not resource IDs."""
    groups = []
    for section in table.entries:
        if section.type_id != 0x00eb0005:
            continue
        o = section.f0
        h = struct.unpack_from('>8I', raw, o)
        if not h[1] or (h[4] - h[3]) != h[1] * 32:
            raise ValueError('Unexpected global model material layout')
        for j in range(h[1]):
            v = struct.unpack_from('>8I', raw, o+h[3]+j*32)
            at = o+v[0]
            kind = raw[at:raw.index(b'\0', at)].decode('ascii')
            if kind == 'Name':
                groups.append({})
            if kind in channels:
                groups[-1][kind] = (v[4] << 32) | v[5]
    return groups


def presentation_meshes(metadata):
    """Every mesh of the global model, as retail draws the whole model.

    Industrial's model (DIST_Water) carries the sea surface itself
    (`ocean.default`, about 16 x 13 km at y -7.7..-3.3), the harbour's
    `ocean.reflection` sheets, distant shore and pier geometry
    (`environment.*`) that is in no district stream, and the tree wall.
    An earlier shader whitelist kept only the trees and reflection sheets, so
    the sea itself was never exported.
    """
    return list(enumerate(metadata))


_CELL = re.compile(r'cPres_(-?\d+)_(-?\d+)_high')


def stream_cell(name):
    """Grid cell (x, z) of a `cPres_<x>_<z>_high[...]` stream file, None for unpaired files (`cPres_Global_proxy`)."""
    m = _CELL.search(name)
    return (int(m.group(1)), int(m.group(2))) if m else None


def proxy_drawn_files(proxy_files, district_files):
    """Proxy stream files retail draws while every full-detail cell is loaded.

    Retail pairs each full-detail cell `cPres_X_Z_high` with the proxy cell
    `cPres_X_Z_high_proxy` (proxy world manager, format "cPres_%d_%d_high_%s",
    TU3 sub_8247EF40) and, when the streamer activates the full cell,
    deactivates its proxy partner (sub_8247BB50 -> sub_82C985A8: activate
    0x4C5724D2 full cell, deactivate 0x4158EE18 proxy cell). The engine keeps
    every district cell loaded, so the retail result is: a proxy cell draws
    only when it has no full-detail partner (Industrial's 59 south-hill cells,
    none in DownTown or University). Unpaired files (`cPres_Global_proxy`) are
    never swapped, so they stay.
    """
    full = {stream_cell(n) for n in district_files} - {None}
    return sorted(n for n in proxy_files if stream_cell(n) not in full)


def proxy_texture_keys(textures, models):
    """The proxy stream's Tex table lists its textures with bit 63 of the
    asset id set (0xAC70170A... for the 0x2C70170A... its materials name), so
    key each one by the id the materials use; textures no drawn model uses are
    left out."""
    used = {n for m in models for mesh in m['meshes']
            for n in [mesh.get('texture_id'), *mesh.get('retail_texture_ids', {}).values()] if n}
    keyed = {}
    for key, value in textures.items():
        plain = f'0x{int(key, 16) & ~(1 << 63):016x}'
        key = plain if key not in used and plain in used else key
        if key in used:
            keyed[key] = value
    return keyed


def convert_proxy(game_root, output, label, report=print):
    """Export the far-proxy terrain retail draws (see `proxy_drawn_files`) to
    `private/native-backdrops/<map>.proxy.skate`; returns the number of drawn
    proxy models, 0 when the district has no proxy stream or no unpaired cell."""
    content = game_root/'data/content'
    proxy_big = next((p for p in content.glob('proxy*_100_Proxy.big')
                      if p.stem.lower() == f'proxy{label}_100_proxy'.lower()), None)
    district_big = next((p for p in content.glob('worldDIST_*.big')
                         if p.stem.lower() == f'worlddist_{label}'.lower()), None)
    target = output/(label+'.proxy.skate')
    target.unlink(missing_ok=True)
    if proxy_big is None or district_big is None:
        return 0
    district_files = [Path(e.path).name for e in BigArchive(district_big).entries if '/cPres_' in e.path]
    archive = BigArchive(proxy_big)
    stream_name = proxy_big.stem.removeprefix('proxy')
    drawn = set(proxy_drawn_files([Path(e.path).name for e in archive.entries
                                   if Path(e.path).name.startswith('cPres_')], district_files))
    if not any(stream_cell(n) for n in drawn):
        return 0
    tools = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(tools/'vendor/university/tools/vanilla_map_extraction/tools'))
    import prepare_hawaiian_dream
    prepare = prepare_hawaiian_dream.prepare
    load = prepare_hawaiian_dream.load_district_stream

    def presentation_only(directory, kind, name):
        # Proxy streams carry presentation only: no Sim table of contents.
        if kind == 'Sim' and not (Path(directory)/f'{name}_Sim.xst').is_file():
            return []
        return load(directory, kind, name)
    prepare_hawaiian_dream.load_district_stream = presentation_only
    try:
        return _write_proxy(prepare, archive, stream_name, label, drawn, target, tools, report)
    finally:
        prepare_hawaiian_dream.load_district_stream = load


def _write_proxy(prepare, archive, stream_name, label, drawn, target, tools, report):
    with tempfile.TemporaryDirectory(prefix='skate-proxy-') as temporary:
        root = Path(temporary)
        archive.extract_entries(archive.entries, root/'raw')
        stream = root/'raw/data/content/world/stream'/stream_name
        manifest_path = prepare(stream_directory=stream, output_root=root/'intermediate', utt_root=tools/'vendor/utt',
            district_name=stream_name, map_name=label, package_name='Skate 3 owned disc',
            cache_format='skate3-rust-map-v1',
            # Proxy textures sit in the stream's own Tex table (no cTex_ files).
            texture_stream_names=('Tex',) if (stream/f'{stream_name}_Tex.xst').is_file() else (),
            raw_texture_cache=True, write_render_sources=False)
        manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
        manifest['models'] = [m for m in manifest['models'] if m['stream_file'] in drawn]
        manifest['grind_splines'] = []
        manifest['textures'] = proxy_texture_keys(manifest['textures'], manifest['models'])
        manifest_path.write_text(json.dumps(manifest), encoding='utf-8')
        write(manifest_path, target, None, render_only=True)
    report(f'{label}: proxy terrain {len(manifest["models"])} models from {len(drawn)} unpaired proxy files')
    return len(manifest['models'])


def convert(game_root, assets, converted):
    vendor = Path(__file__).resolve().parents[1] / 'vendor'
    sys.path.insert(0, str(vendor / 'utt'))
    sys.path.insert(0, str(vendor / 'university/tools/vanilla_map_extraction/tools'))
    import numpy as np
    import mdl_parser
    import rx2_parser
    import prepare_hawaiian_dream as prep
    from retail_lightmap_uv import decode_lightmap_uvs
    from retail_texture_decode import B5G6R5_FORMAT_ID, decode_b5g6r5

    archive = BigArchive(game_root / 'data/big/miscload.big')
    entries = {e.path: e for e in archive.entries}
    collections = Collections(converted)
    output = assets / 'private/native-backdrops'
    output.mkdir(parents=True, exist_ok=True)
    available = {p.stem.removeprefix('world') for p in (game_root/'data/content').glob('worldDIST_*.big')}
    count = 0
    for (cls, _), row in collections.rows.items():
        if cls != key_hash('world'):
            continue
        own_stream = next((v['data'] for k, v in row['fields'].items()
                           if key_hash(k) == key_hash('WorldStream')), None)
        if own_stream not in available:
            continue
        fields, _ = collections.resolve('world', row['key'])
        # These global presentation resources are separate from WorldStream.
        if key_hash('Hash_951898F6C0FA6856') not in fields:
            continue
        model_name = field(fields, 'Hash_951898F6C0FA6856')
        if not model_name:
            continue
        texture_name = field(fields, 'Hash_CA5A157A65E75934')
        raw = archive.read(entries['data/content/' + model_name + '.rx2'])
        model = mdl_parser.parse_rx2(raw)
        groups, bindings = prep._bind_material_groups_by_guid(
            raw, prep._group_material_parameters(model.materials), len(model.meshes),
            allow_import_order_fallback=False)
        selected = presentation_meshes([prep._material_metadata(groups, i) for i in range(len(model.meshes))])
        if not selected:
            continue
        textures = rx2_parser.parse_rx2(archive.read(entries['data/content/' + texture_name + '.rx2']))
        table = rx2_parser.RX2File(raw)
        table.parse()
        channel_groups = texture_groups(raw, table, prep.RETAIL_TEXTURE_CHANNELS)
        with tempfile.TemporaryDirectory(prefix='skate-backdrop-') as temporary:
            root = Path(temporary)
            arrays, meshes, exported_textures = {}, [], {}
            for i, material in selected:
                mesh = model.meshes[i]
                binding = bindings[i]
                meshes.append(dict(material, index=i, name=groups[i]['Name'][0],
                    retail_material_guid=f"0x{binding['material_guid']:016X}",
                    retail_material_handle=f"0x{binding['material_handle']:08X}",
                    retail_material_group_index=binding['group_index'], source_offsets=mesh.source_offsets))
                arrays[f'vertices_{i}'] = mesh.vertices
                arrays[f'faces_{i}'] = mesh.faces
                arrays[f'uvs_{i}'] = mesh.uvs
                arrays[f'normals_{i}'] = mesh.normals
                lm = decode_lightmap_uvs(raw, vertex_buffer_offset=mesh.source_offsets['vertex_buffer'],
                    vertex_count=mesh.vertex_count, vertex_stride=mesh.vertex_stride, attributes=mesh.attributes)
                if lm is not None:
                    arrays[f'lightmap_uvs_{i}'] = lm.values
                for role, name in material['retail_texture_ids'].items():
                    if name in exported_textures:
                        continue
                    texture = _texture(textures, channel_groups[binding['group_index']][role])
                    rgba = texture.rgba
                    if texture.fmt_id == B5G6R5_FORMAT_ID:
                        rgba = decode_b5g6r5(textures.data[texture.data_offset:texture.data_offset+texture.buffer_size],
                                            texture.width, texture.height)
                    path = name + '.rgba'
                    (root/path).write_bytes(rgba)
                    exported_textures[name] = dict(width=texture.width, height=texture.height, rgba=path)
            np.savez(root/'model.npz', **arrays)
            name = own_stream.removeprefix('DIST_')
            manifest = dict(map_name=name, district_name=own_stream, models=[dict(
                asset_id='0x'+hashlib.sha256(raw).hexdigest()[:16], npz='model.npz', meshes=meshes)],
                textures=exported_textures, normal_texture_policy=dict(excluded_texture_ids=[]), grind_splines=[],
                source_model=model_name, source_textures=texture_name, source_sha256=hashlib.sha256(raw).hexdigest())
            path = root/'manifest.json'
            path.write_text(json.dumps(manifest))
            write(path, output/(name+'.skate'), None, render_only=True)
            count += 1
        convert_proxy(game_root, output, name)
    return count


if __name__ == '__main__':
    import argparse
    from .vlt import convert as convert_vlt
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-root', type=Path, required=True)
    parser.add_argument('--assets', type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='skate-backdrop-db-') as work:
        database = BigArchive(args.game_root/'data/big/db.big')
        needed = {'skaterschema.bin', 'skaterschema.vlt', 'skatercollections.bin', 'skatercollections.vlt'}
        database.extract_entries([e for e in database.entries if Path(e.path).name.lower() in needed], Path(work))
        stem = Path(work)/'data/db'
        names = (Path(__file__).parent/'names.txt').read_text(encoding='utf-8').splitlines()
        converted = convert_vlt(stem/'skaterschema', stem/'skatercollections', names)
        print('Prepared', convert(args.game_root, args.assets, converted), 'global presentation backdrops')
