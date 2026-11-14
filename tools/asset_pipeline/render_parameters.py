"""Export complete authored parameter rows for additional presentation shaders."""
import json
import struct
from .environment import Collections, key_hash


def exposure_parameters(collections, location):
    fields, chain = collections.resolve('render_locations', location)
    key = int(fields[key_hash('Hash_C345507C4B9B6F62')]['data'][:16], 16)
    rendering, rendering_chain = collections.resolve('rendering', key)
    values = {n: struct.unpack('>f', bytes.fromhex(rendering[key_hash('auto_exposure_'+n)]['data']))[0]
              for n in ('target_luminance', 'min', 'max', 'damping')}
    return dict(values, location_chain=chain, rendering_chain=rendering_chain)


def convert(assets, converted):
    collections = Collections(converted)
    result = {}
    aliases = ('default', 'reflection', 'backlituvscroll', 'transparent',
               'flowing', 'flowingalpha', 'alpha', 'skatepark', 'videoscreen', 'alphatest', 'flag', 'diffuse', 'decal')
    for (cls, key), row in collections.rows.items():
        # dynamicobject: the props' m_params (dynamicobject_defaultPS c14..c15).
        # advertisement: m_params.y = 0.35 scales billboards (advertisement_defaultPS slot 34).
        # animated.flag: the cloth sway amplitudes, frequencies and phases (vertexanimate_defaultVS c8..c10).
        # environmentpark: the skate.Park pieces' c18 (.y the fog scale; alphatest row 1 its ambient, research b443).
        for family in ('water', 'ocean', 'incandescent', 'dynamicobject', 'advertisement', 'animated', 'environmentpark'):
            if cls != key_hash('material_' + family):
                continue
            fields, _ = collections.resolve(cls, key)
            raw = fields.get(key_hash('m_params'), {}).get('array', {}).get('items', [])
            if not raw:
                continue
            name = next((n for n in aliases if key_hash(n) == key), row['key'])
            result[family + '.' + name] = [struct.unpack('>4f', bytes.fromhex(r)) for r in raw]
    # g_ViewDotLight inputs (sub_828012D0): light direction, (bias, scale), (tree floor, light floor). Every disc has
    # the record; without it (partial or synthetic data) the game keeps the retail values (`ViewDotLightParams`).
    try:
        fields, _ = collections.resolve('rendering', 'default')
    except KeyError:
        fields = {}
    rows = []
    for name, count in (('Hash_D900AEF7EA4D75A2', 4), ('Hash_222CBA2D46EA6839', 4),
                        ('Hash_9CDB53D164E5DA30', 1), ('Hash_0279CCE002B46BC4', 1)):
        field = fields.get(key_hash(name), {})
        data = (field.get('array', {}).get('items') or [field.get('data', '')])[0]
        rows.append(list(struct.unpack('>%df' % count, bytes.fromhex(data[:8 * count]))) if data else None)
    if all(rows):
        result['rendering.default'] = [rows[0], rows[1], rows[2] + rows[3] + [0.0, 0.0]]
    path = assets / 'private/render-parameters.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result), encoding='utf-8')
    fields, _ = collections.resolve('Hash_1FFDC8E3ACA07C1F', 'auto_exposure')
    exposure = {n: struct.unpack('>f', bytes.fromhex(fields[key_hash('auto_exposure_'+n)]['data']))[0]
                for n in ('target_luminance', 'min', 'max', 'damping')}
    (assets / 'private/exposure.json').write_text(json.dumps(exposure), encoding='utf-8')
    profiles = {}
    for (cls, key), row in collections.rows.items():
        if cls != key_hash('world'):
            continue
        own_stream = next((v['data'] for k, v in row['fields'].items() if key_hash(k) == key_hash('WorldStream')), None)
        if not own_stream or not own_stream.startswith('DIST_'):
            continue
        world, _ = collections.resolve(cls, key)
        location = int(world[key_hash('Hash_7E23D10785C43717')]['data'][:16], 16)
        profiles[own_stream.removeprefix('DIST_').casefold()] = exposure_parameters(collections, location)
    (assets / 'private/exposure-profiles.json').write_text(json.dumps(profiles, indent=2), encoding='utf-8')
    return len(result)
