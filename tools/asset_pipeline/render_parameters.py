"""Export complete authored parameter rows for additional presentation shaders."""
import json
import struct
from .environment import Collections, key_hash


def convert(assets, converted):
    collections = Collections(converted)
    result = {}
    aliases = ('default', 'reflection', 'backlituvscroll', 'transparent',
               'flowing', 'flowingalpha', 'alpha', 'skatepark', 'videoscreen')
    for (cls, key), row in collections.rows.items():
        for family in ('water', 'ocean', 'incandescent'):
            if cls != key_hash('material_' + family):
                continue
            fields, _ = collections.resolve(cls, key)
            raw = fields.get(key_hash('m_params'), {}).get('array', {}).get('items', [])
            if not raw:
                continue
            name = next((n for n in aliases if key_hash(n) == key), row['key'])
            result[family + '.' + name] = [struct.unpack('>4f', bytes.fromhex(r)) for r in raw]
    path = assets / 'private/render-parameters.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result), encoding='utf-8')
    fields, _ = collections.resolve('Hash_1FFDC8E3ACA07C1F', 'auto_exposure')
    exposure = {n: struct.unpack('>f', bytes.fromhex(fields[key_hash('auto_exposure_'+n)]['data']))[0]
                for n in ('target_luminance', 'min', 'max', 'damping')}
    (assets / 'private/exposure.json').write_text(json.dumps(exposure), encoding='utf-8')
    return len(result)
