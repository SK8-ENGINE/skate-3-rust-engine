"""Living world, pedestrian half (setup group ``livingworld``): tables, census grids, road network,
waypoints and the ped model manifest, read from the user's disc at setup time.

Outputs (under ``<assets>/private/living_world/``; the NPC skater half writes its own files next to
them, see ``living_world_skaters.py``):

``tables.json``
    Every living-world vault class (``skatercollections``) with inheritance applied:
    ``{"version", "classes": {class: {record: {"parent", "fields": {name: value}}}}, "field_names"}``.
    Field names are readable where the research notes name them (``FIELD_NAMES``) and ``Hash_*``
    otherwise; ``field_names`` maps every readable name back to its hash so either spelling can be
    patched. Struct values are decoded field by field (``STRUCTS``), RefSpecs become
    ``{"class", "key"}`` (``None`` when empty). A content overlay patches records by
    ``class/record/field``.

``<District>.census.bin`` + ``census.json``
    The world-painter census layers (``livingworld_npc_census``, ``livingworld_vehicle_census``)
    sampled on a regular grid (format: ``write_census_grid``). ``census.json`` lists per district the
    file, bounds, cell size, layers and record names.

``roads.json`` + ``roads.bin`` + ``roads_raw.bin``
    The road network streams (RW object ``0x00EB0013``), version 2: per district the decoded segments
    (with ``to_node`` / ``from_node``, lanes, speed limit), junctions (approach / exit records, turn
    connectors) and the tile objects; ``roads.bin`` is the little-endian graph with the lane pieces
    (``living_world_roads.write_graph``, parsed by ``skate-data::roads``) and ``roads_raw.bin`` keeps
    every blob verbatim (same container as the skater paths, magic ``LWROADS``).

``waypoints.json``
    Waypoint groups (RW object ``0x00EB001A``; ``parse_waypoints``): per group its box, ids, type
    (``waypoint_vendingmachine``, ``waypoint_usetrashbin``) and its waypoints (position, facing,
    locator name).

``navmesh.json`` + ``navmesh.bin``
    The NavPower nav graphs (``0x00EB0027``) of each district, decoded and joined across tiles
    (``living_world_navmesh``, peds M3); the JSON holds counts and the area histogram.

``vehicles.json`` + ``vehicles/<recipe>.glb``
    The car models, palettes, vehicle entities and vehicle census (``living_world_vehicles.py``).

``models.json`` + ``models/<recipe>.glb``
    The binary ``.recipe`` files of ``livingworld.big`` (``parse_recipe``): per recipe its parts,
    both LODs, model arenas, materials and textures, with a check that each arena exists, and the
    glTF model built from it (``living_world_models.py``; ``glb`` entry: status, file, bones, LOD
    vertex / triangle counts).

Entry points: :func:`export` (peds), :func:`export_group` (both halves, for the setup group) and
:func:`validate`. Values come from the user's disc at setup time only.
"""
from __future__ import annotations

import hashlib
import json
import struct
import sys
from pathlib import Path

VERSION = 1
ROADS_VERSION = 2  # roads.json / roads.bin: v2 adds lanes, junctions and connectors (living_world_roads)
ROADDATA, WAYPOINTDATA, NAVPOWERDATA = 0x00EB0013, 0x00EB001A, 0x00EB0027
CENSUS_LAYERS = ('livingworld_npc_census', 'livingworld_vehicle_census')
CENSUS_CELL = 4.0  # m: the road network's lane spacing; leaf cells of the painter are larger
CENSUS_MAGIC, ROADS_MAGIC = b'LWCENSUS', b'LWROADS\0'
RECIPE_VERSION = 7
CITY_DISTRICTS = ('DownTown', 'Industrial', 'University')

# Vault classes exported to tables.json (design 27 section 2, item 1).
TABLE_CLASSES = (
    'livingworld', 'livingworld_census', 'livingworld_census_ranges', 'livingworld_categorygroups',
    'livingworld_entitycategories', 'livingworld_entities', 'livingworld_entities_chase',
    'livingworld_entities_perceptions', 'livingworld_entities_navigation', 'livingworld_entities_locomotion',
    'livingworld_entities_moodreactions', 'livingworld_entities_moodresults', 'livingworld_entities_knowledge',
    'livingworld_entities_protect', 'livingworld_entities_patrolzone', 'livingworld_moodeventcategories',
    'livingworld_models', 'livingworld_entity_animation', 'livingworld_entity_animation_postadjust',
    'livingworld_entity_takedown', 'livingworld_entity_headtracking', 'livingworld_handprops',
    'livingworld_handprops_usagecharacteristics', 'livingworld_conversations',
    'livingworld_conversation_categories', 'livingworld_conversation_category_groups',
    'livingworld_load_groups', 'livingworld_props', 'physics_ai',
    'livingworld_vehicle_characteristics', 'livingworld_vehicle_drivers',
    # Hand prop models: livingworld_handprops.model -> dmo_models (first field = the DMO template id, 82E3DE18; b87 / b89).
    'dmo_models')

# Readable names for fields the research identified (.claude/notes/npc-livingworld-re.md,
# peds-re.md). Everything else keeps its Hash_ name. Tags: [data] read from the vault values,
# [code] from the reading code site.
FIELD_NAMES = {
    'livingworld': {
        # trafficlights record, read by sub_826B1540 for each of its 4 signal controllers [code]: phases
        # all-red, green, amber, all-red, then red while the other light group runs green + amber.
        'Hash_3836077DEFB6F008': 'signal_green',          # 7.0 s (phase state 2) [code + data]
        'Hash_CFCA94D67E0B234E': 'signal_amber',          # 1.0 s (phase state 1) [code + data]
        'Hash_2C0CE3766A0074CD': 'signal_all_red',        # 0.5 s (phase state 0) [code + data]
    },
    'livingworld_census': {
        'Hash_98F6489E23ED2661': 'entry',                 # tLWCensusEntry: category group + max population [data]
        'Hash_FFE5E258BD468196': 'vehicle_extra',          # vehicles only, read at sub_826B7EE4 [code]
    },
    'livingworld_census_ranges': {
        'Hash_E0C6647F889A71FC': 'circle_slow',            # set A, lerped by player km/h (sub_826B7D60) [code]
        'Hash_D993B45FFEB335F6': 'circle_fast',            # set B
    },
    'livingworld_categorygroups': {'Hash_D5E1267E2D715124': 'categories'},
    'livingworld_entitycategories': {'Hash_D5E1267E2D715124': 'entities'},
    'livingworld_conversation_categories': {'Hash_D5E1267E2D715124': 'entities'},
    'livingworld_conversation_category_groups': {'Hash_D5E1267E2D715124': 'categories'},
    'livingworld_load_groups': {'Hash_69732379A3E7758C': 'models'},
    'livingworld_entities': {
        'Model': 'model',
        'Hash_864A7084CA220523': 'handprops',              # tLWHandPropUsage list [data]
        'Hash_C04236FB548697D0': 'collision_gate_a',        # first-hit magnitude gates (sub_82E38FB8) [code]
        'Hash_797AA1D5F828819B': 'collision_gate_b',
        'Hash_942AB8AEE4B414ED': 'name',                     # the record's own name [data]
        'Hash_BD8F02BA1986215A': 'display_name',             # e.g. "Skater Female" [data]
        'Hash_CA4D0AD4EA45965B': 'ai_graph',                 # state graph path, e.g. .../Pedestrian.xml [data]
        'Hash_46B836EE959C0238': 'handprop_odds',            # (livingworld_handprops, probability) pairs [data]
        'Hash_F89323E420A6AAA3': 'plugin_odds',              # (waypoint_* plugin entity, probability) pairs [data]
        'Hash_023EF929823A3C50': 'driver',                   # vehicles: RefSpec livingworld_vehicle_drivers [data]
        'Hash_92A043B4A11F1A2A': 'spec',                     # vehicles: RefSpec livingworld_vehicle_characteristics [data]
        'Hash_E356ED00ABF1D7F0': 'scoring',                  # RefSpec scoring_entities (vehicles: car) [data]
    },
    'livingworld_vehicle_characteristics': {
        'Hash_BA2DDD830C731EE4': 'engine_audio',             # RefSpec aud_traffic_engine (the traffic engine sound) [data]
        'Hash_543475921FD9E04A': 'alarm_impulse',            # contact impulse that starts the alarm (sub_82C3C150) [code]
        'Hash_E199FC7CEA222809': 'alarm_duration',           # alarm length in s (sub_82C3A4D0) [code]
        'Hash_D20826F15FB15A2E': 'follow_min_speed_kmh',     # following rule only when both speeds exceed it (sub_82C3FA08) [code]
        'Hash_256A412E350A2659': 'skater_follow_margin_kmh',  # skater speed minus this x 0.2778 (sub_82C3FA08, b63) [code]
        'Hash_3AB7FC7CF7A17C81': 'skater_far_distance',      # FAR rule braking distance, m (2 x D + 0.001; b63) [code]
        'Hash_33466832D8178EAF': 'skater_scan_range',        # skater scan range and rear zone length, m (sub_82C414A8) [code]
        'Hash_D49FC49019181EE5': 'skater_near_range',        # NEAR rule range, m (sub_82C3FA08) [code]
        'Hash_F682D359CDBC4D12': 'skater_near_distance',     # NEAR rule braking distance, m [code]
        'Hash_4727CF785EF735C8': 'release_grace',            # s without the FAR rule after a skitch release (sub_82C34B30) [code]
    },
    'livingworld_vehicle_drivers': {
        'Hash_FE83E2E0A19A9AFE': 'honk_obstacle_time',       # obstacle ahead this long -> horn kind 2 (sub_82C40660) [code]
        'Hash_BC1A827C21919C3B': 'honk_blocked_time',        # blocked this long -> horn kind 4 / 5 (sub_82C40660) [code]
        'Hash_40540E1A0A5D447A': 'honk_approach_speed_kmh',  # approaching an obstacle faster -> horn kind 1 (sub_82C34190) [code]
        'Hash_559BA807F95FF93E': 'pull_over_chance',         # per manoeuvre decision (sub_82C41CD0) [code]
        'Hash_988BB0F6F043EB3D': 'parked_time',              # s parked before pulling out (sub_82C3A3A8) [code]
    },
    'livingworld_entities_chase': {
        'Hash_240EB11A0CB469C3': 'escape_distance',         # ChaseeEscaped sub_826AC668 [code]
        'Hash_4686FEC68A60BD69': 'exhaustion_limit',        # NeedToRest sub_826AC898 [code]
        'Hash_78E2E1B19721FD4E': 'give_up_after_takedowns',  # sub_826AD690 [code]
        'Hash_314597EDA6B921F2': 'alert_distance',
        'Hash_FA762B2EA065A6D7': 'alert_timer',
        'Hash_5A3C4A7700DDA13C': 'unreachable_timer',
    },
    'livingworld_models': {
        'Hash_62F7C585686F3371': 'recipe',                  # recipe name in livingworld.big [data]
        'Hash_3EB8E0CD15F0891C': 'voice',                   # Sk8::Audio::eSk8Characters (speech voice) [data]
        'Hash_F6897CC7B637850C': 'category',                # eLivingWorldModelCategory: 0 peds, 1 marquee, 2 props, 3 vehicles, 4 DMOs [data]
        'Hash_DF76D7D773857EDB': 'secondary_colours',       # Vector4 list; base record (1, 0, 0) = the red mask channel [data]
        'Hash_12026E2EED18CC8D': 'chassis_colours',         # Vector4 list (taxi: yellow, red); base (0, 0, 1) = the blue paint area [data]
    },
    'livingworld_entity_animation': {
        'Hash_541FFA2E9D81C947': 'knockdown_speed_a',        # kind 0 above this (sub_82E38FB8) [code]
        'Hash_7C5E39ECE5A5572E': 'knockdown_speed_b',
        'Hash_081F978F8706D77B': 'knockdown_speed_flagged_a',
        'Hash_5E566360FDC5FDD1': 'knockdown_speed_flagged_b',
        'Hash_74C0B72217A7704E': 'recovery_intervals',       # re-hit posture by get-up time (sub_8269E460) [code]
        'Hash_E37F00409DEAB4D2': 'recovery_intervals_b',
        'Hash_0162B7A9B0C086EE': 'recovery_intervals_left',
    },
    'livingworld_entity_takedown': {'Hash_76AC331FBD5803FF': 'takedowns'},
    'livingworld_handprops': {'Model': 'model'},
    'livingworld_handprops_usagecharacteristics': {'Hash_E4E9C4E514981944': 'actions'},
    'livingworld_entities_moodreactions': {
        'Hash_FA4C8C3BFCC722BC': 'prerequisites',
        'Hash_4694B0039EAACDB8': 'results',
    },
}

# Struct layouts: (name, format, offset). Unnamed words keep their offset (``u32_24``) so the
# key is stable once a meaning is found. ``ref`` = 16-byte RefSpec (class hash, record hash).
STRUCTS = {
    'Sk8::LivingWorld::tLWCensusEntry': (('group', 'ref', 0), ('max_population', 'I', 24)),
    'Sk8::LivingWorld::tLWGroupEntry': (('category', 'ref', 0), ('weight', 'f', 24)),
    'Sk8::LivingWorld::tLWRefspecProbabilityPair': (('ref', 'ref', 0), ('probability', 'f', 24)),
    'Sk8::LivingWorld::tLWRefspecPriorityPair': (('ref', 'ref', 0), ('priority', 'I', 24)),
    'Sk8::LivingWorld::tLWCensusCircle': (('spawn_inner', 'f', 0), ('spawn_outer', 'f', 4), ('cull', 'f', 8),
                                          ('forward_offset', 'f', 12), ('speed_kmh', 'f', 16)),
    'Sk8::LivingWorld::tLWHandPropUsage': (('handprop', 'ref', 0), ('f32_24', 'f', 24), ('f32_28', 'f', 28),
                                           ('usage', 'ref', 32)),
    'Sk8::LivingWorld::tLWHandPropUsageCharacteristics': (('action', 'I', 0), ('f32_4', 'f', 4),
                                                          ('f32_8', 'f', 8), ('f32_12', 'f', 12)),
    'Sk8::LivingWorld::tLWMoodEventPrerequisites': (('event', 'ref', 0), ('u32_24', 'I', 24), ('u32_28', 'I', 28),
                                                    ('u32_32', 'I', 32), ('u32_36', 'I', 36)),
    'Sk8::LivingWorld::tLWOpacityDistance': (('f32_0', 'f', 0), ('f32_4', 'f', 4), ('f32_8', 'f', 8)),
    'Sk8::LivingWorld::tLWCollisionCapsuleParams': (('f32_0', 'f', 0), ('f32_4', 'f', 4), ('f32_8', 'f', 8)),
    'Sk8::LivingWorld::tRecoveryCollisionIntervals': (('lying_down_until', 'f', 0), ('crouched_until', 'f', 4)),
    'Sk8::LivingWorld::tPedestrianBoneAdjustMap': (('u32_0', 'I', 0), ('u32_4', 'I', 4)),
    'Sk8::LivingWorld::tLocomotionSpeedIntentMapping': tuple((f'u32_{4 * i}', 'I', 4 * i) for i in range(8)),
    'Anim::tAnimAttributes': (('anim', 'I', 0), ('u32_4', 'I', 4), ('i32_8', 'i', 8), ('i32_12', 'i', 12),
                              ('u32_16', 'I', 16), ('window_0', 'w', 20), ('window_1', 'w', 32), ('window_2', 'w', 44)),
    'Anim::tAnimTakedown': (('anim', 'I', 0), ('anim_b', 'I', 4), ('f32_8', 'f', 8), ('f32_12', 'f', 12),
                            ('reach', 'f', 16), ('angle_min', 'f', 20), ('angle_max', 'f', 24), ('u32_28', 'I', 28),
                            ('u32_32', 'I', 32), ('u32_36', 'I', 36)),
    'Math::Vector3': (('x', 'f', 0), ('y', 'f', 4), ('z', 'f', 8)),
    'Attrib::Types::Vector4': (('x', 'f', 0), ('y', 'f', 4), ('z', 'f', 8), ('w', 'f', 12)),
}


def _round(value: float) -> float:
    """The shortest decimal that reads back as the same 32-bit float (3.167, not 3.1670001)."""
    if value != value or value in (float('inf'), float('-inf')):
        return value
    raw = struct.pack('>f', value)
    for digits in range(6, 10):
        short = float(f'{value:.{digits}g}')
        if struct.pack('>f', short) == raw:
            return short
    return value


class Refs:
    """Class and record names by their 64-bit vault hash."""

    def __init__(self, collections: list[dict]):
        from .vlt import hash64
        self.classes = {hash64(c): c for c in {r['class'] for r in collections}}
        self.records = {}
        for row in collections:
            self.records.setdefault(row['class'], {})[hash64(row['key'])] = row['key']

    def ref(self, raw: bytes):
        class_key, record_key = struct.unpack_from('>QQ', raw, 0)
        if not record_key:
            return None
        cls = self.classes.get(class_key, f'Hash_{class_key:016X}')
        return {'class': cls, 'key': self.records.get(cls, {}).get(record_key, f'Hash_{record_key:016X}')}


def decode_struct(kind: str, raw: bytes, refs: Refs):
    layout = STRUCTS.get(kind)
    if layout is None:
        return None
    out = {}
    for name, fmt, offset in layout:
        if fmt == 'ref':
            out[name] = refs.ref(raw[offset:offset + 16]) if offset + 16 <= len(raw) else None
        elif fmt == 'w':  # timing window: start, end (f32), tag (i32)
            start, end, tag = struct.unpack_from('>ffi', raw, offset)
            out[name] = [_round(start), _round(end), tag]
        else:
            value = struct.unpack_from('>' + fmt, raw, offset)[0]
            out[name] = _round(value) if fmt == 'f' else value
    return out


def decode_field(field: dict, refs: Refs):
    """One converted vault field as JSON (numbers, bools, text, refs, structs, arrays)."""
    kind = field.get('type', '')
    if 'array' in field:
        array = field['array']
        if kind == 'EA::Reflection::Text':
            return list(array.get('text_items', []))
        return [decode_field({'type': kind, 'data': item}, refs) for item in array.get('items', [])]
    data = field.get('data')
    if kind == 'EA::Reflection::Text':
        return data
    raw = bytes.fromhex(data) if isinstance(data, str) else b''
    if kind == 'EA::Reflection::Float':
        return _round(struct.unpack('>f', raw[:4])[0])
    if kind == 'EA::Reflection::Bool':
        return raw[:1] != b'\0'
    if kind == 'EA::Reflection::Int8':
        return struct.unpack('>b', raw[:1])[0]
    if kind == 'EA::Reflection::UInt8':
        return raw[0]
    if kind in ('EA::Reflection::Int32', 'EA::Reflection::UInt32'):
        return struct.unpack('>i' if kind.endswith('Int32') and 'U' not in kind else '>I', raw[:4])[0]
    if kind == 'Attrib::RefSpec' or kind.startswith('Attrib::Gen::ClassRefSpec_'):
        return refs.ref(raw)
    decoded = decode_struct(kind, raw, refs)
    if decoded is not None:
        return decoded
    # 32-bit enums (eSk8Characters, eMoodResult, eConversationTopic, ...): their value.
    if len(raw) == 4 and kind.rsplit('::', 1)[-1][:1] in ('e', 'E'):
        return struct.unpack('>i', raw)[0]
    return {'type': kind, 'hex': data}


def tables(collections: list[dict]) -> dict:
    """tables.json content: every TABLE_CLASSES record with inheritance applied."""
    refs = Refs(collections)
    by_class: dict[str, dict[str, dict]] = {}
    for row in collections:
        if row['class'] in TABLE_CLASSES:
            by_class.setdefault(row['class'], {})[row['key']] = row
    classes, field_names = {}, {}
    for cls in TABLE_CLASSES:
        rows = by_class.get(cls, {})
        names = FIELD_NAMES.get(cls, {})
        field_names[cls] = {readable: hashed for hashed, readable in names.items()}
        out = {}
        for key, row in sorted(rows.items()):
            chain, node = [], row
            while node:
                if node['key'] in [c['key'] for c in chain]:
                    raise ValueError(f'cyclic {cls} inheritance at {key}')
                chain.append(node)
                node = rows.get(node.get('parent', ''))
            merged = {}
            for node in reversed(chain):
                merged.update(node['fields'])
            out[key] = {'parent': row.get('parent', '') or None,
                        'fields': {names.get(name, name): decode_field(field, refs)
                                   for name, field in sorted(merged.items())}}
        classes[cls] = out
    return {'version': VERSION, 'classes': classes, 'field_names': field_names}


# ---------------------------------------------------------------- census chain

def model_recipes(doc: dict, model: str | None) -> list[str]:
    """The recipes a ``livingworld_models`` record stands for: its own recipe, or, for a group record
    without one (``skater_female``, ``worker_male``: entities point at these), the recipes of the
    records that inherit from it [data; how retail picks among them is not read from the code yet]."""
    models = doc['classes']['livingworld_models']
    if not model or model not in models:
        return []
    own = models[model]['fields'].get('recipe')
    if own:
        return [own]
    return sorted(r['fields']['recipe'] for r in models.values()
                  if r['parent'] == model and r['fields'].get('recipe'))


def census_chain(doc: dict, census_record: str) -> dict:
    """Resolve one census record to {group, max_population, categories: [{category, weight,
    entities: [{entity, model, recipe}]}]} through the exported tables."""
    c = doc['classes']
    entry = c['livingworld_census'][census_record]['fields'].get('entry') or {}
    group = (entry.get('group') or {}).get('key')
    result = {'census': census_record, 'group': group, 'max_population': entry.get('max_population'),
              'categories': []}
    groups = c['livingworld_categorygroups']
    for item in (groups.get(group, {}).get('fields', {}).get('categories') or []):
        category = (item.get('category') or {}).get('key')
        entities = []
        for ref in (c['livingworld_entitycategories'].get(category, {}).get('fields', {}).get('entities') or []):
            if not ref:
                continue
            entity = ref['key']
            fields = c['livingworld_entities'].get(entity, {}).get('fields', {})
            model = (fields.get('model') or {}).get('key')
            entities.append({'entity': entity, 'model': model, 'recipes': model_recipes(doc, model)})
        result['categories'].append({'category': category, 'weight': item.get('weight'), 'entities': entities})
    return result


# ---------------------------------------------------------------- district streams

def district_assets(game_root: Path, work: Path, districts=CITY_DISTRICTS):
    """Yield (district, tile stream name, asset id, processor id, data) for every asset of the
    districts' ``cSim_*`` streams; copies stored in several cells are read once."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/university/tools/vanilla_map_extraction/tools'))
    from skate3_streams import read_atoc, read_sfil
    from tools.owned_game.big import BigArchive
    for district in districts:
        archive = game_root/'data/content'/f'worldDIST_{district}.big'
        if not archive.is_file():
            continue
        big = BigArchive(archive)
        index = next((e for e in big.entries if e.path.endswith('_Sim.xst')), None)
        if index is None:
            continue
        folder = work/'living_world_streams'/district
        folder.mkdir(parents=True, exist_ok=True)
        (folder/'index.xst').write_bytes(big.read(index))
        records = read_atoc(folder/'index.xst')
        by_id = {r.asset_id: r for r in records}
        seen = set()
        for entry in sorted(big.entries, key=lambda e: e.path):
            name = Path(entry.path).name
            if not (name.startswith('cSim_') and name.endswith('.xsf')):
                continue
            stream = folder/name
            stream.write_bytes(big.read(entry))
            for asset in read_sfil(stream, records, require_all_records=False, record_index=by_id):
                if asset.record.asset_id in seen:
                    continue
                seen.add(asset.record.asset_id)
                yield district, Path(name).stem, asset.record.asset_id, asset.record.processor_id, asset.data
            stream.unlink()


def arena_objects(arena: bytes, object_type: int) -> list[bytes]:
    """Every object of one RW type in an RW4 arena (directory count +0x20, offset +0x30,
    24-byte entries: offset, ?, size, ?, ?, type)."""
    if len(arena) < 0x34:
        return []
    count, directory = struct.unpack_from('>I', arena, 0x20)[0], struct.unpack_from('>I', arena, 0x30)[0]
    if directory + 24 * count > len(arena):
        raise ValueError('arena directory runs past the end')
    out = []
    for i in range(count):
        offset, _, size, _, _, kind = struct.unpack_from('>6I', arena, directory + 24 * i)
        if kind == object_type:
            if offset + size > len(arena):
                raise ValueError(f'object {i} (type {object_type:#x}) runs past the arena end')
            out.append(arena[offset:offset + size])
    return out


# ---------------------------------------------------------------- census grids

def census_record_names(collections_or_names) -> dict[int, str]:
    """Layer keys -> census record names (keys are the vault hash of the record name)."""
    from .vlt import hash64
    names = collections_or_names
    if names and isinstance(names[0], dict):
        names = [r['key'] for r in names if r['class'] == 'livingworld_census']
    return {hash64(n): n for n in names}


def census_grid(layers: dict[str, list[dict]], names: dict[int, str], cell: float = CENSUS_CELL) -> dict:
    """Sample region layers ({layer name: [region layer dicts]}) on a regular x/z grid.
    Cell (i, j) holds the record at its centre: origin + (i + 0.5, j + 0.5) * cell."""
    from .audio_formats import region_key
    boxes = [l['box'] for ls in layers.values() for l in ls]
    if not boxes:
        return {'cell': cell, 'origin': [0.0, 0.0], 'size': [0, 0], 'names': [], 'layers': {}}
    import math
    x0 = math.floor(min(b[0] - b[2] for b in boxes) / cell) * cell
    z0 = math.floor(min(b[1] - b[3] for b in boxes) / cell) * cell
    x1 = math.ceil(max(b[0] + b[2] for b in boxes) / cell) * cell
    z1 = math.ceil(max(b[1] + b[3] for b in boxes) / cell) * cell
    width, height = int(round((x1 - x0) / cell)), int(round((z1 - z0) / cell))
    table: list[str] = []
    index: dict[str, int] = {}
    out = {}
    for layer_name in CENSUS_LAYERS:
        cells = bytearray(2 * width * height)
        for layer in layers.get(layer_name, []):
            cx, cz, hx, hz = layer['box']
            i0 = max(0, int((cx - hx - x0) / cell))
            i1 = min(width, int(math.ceil((cx + hx - x0) / cell)))
            j0 = max(0, int((cz - hz - z0) / cell))
            j1 = min(height, int(math.ceil((cz + hz - z0) / cell)))
            for j in range(j0, j1):
                z = z0 + (j + 0.5) * cell
                for i in range(i0, i1):
                    at = 2 * (j * width + i)
                    if cells[at] or cells[at + 1]:
                        continue  # first tile that paints a cell wins (tiles do not overlap in practice)
                    key = region_key(layer, x0 + (i + 0.5) * cell, z)
                    if key is None:
                        continue
                    name = names.get(key, f'Hash_{key:016X}')
                    if name not in index:
                        table.append(name)
                        index[name] = len(table)
                    struct.pack_into('<H', cells, at, index[name])
        out[layer_name] = bytes(cells)
    return {'cell': cell, 'origin': [x0, z0], 'size': [width, height], 'names': table, 'layers': out}


def write_census_grid(grid: dict) -> bytes:
    """``<District>.census.bin`` (little-endian):

    ====  =====================================================================
    +0    8 B  ``LWCENSUS``
    +8    u32  version (1)
    +12   f32  cell size (m)
    +16   f32  origin x, f32 origin z (world metres; cell (i, j) covers
               [x + i*cell, x + (i+1)*cell) x [z + j*cell, z + (j+1)*cell))
    +24   u32  width (cells along x), u32 height (cells along z)
    +32   u32  layer count L, u32 name count N
    +40   L x 36 B: 32 B layer name (NUL padded), u32 offset of its cells
    then  N names: u16 length + ASCII bytes (record names of ``livingworld_census``)
    then  per layer width*height u16 cells, row-major by z then x, 4-byte aligned;
          0 = unpainted (retail falls back to the ``default`` record), k = name k-1
    ====  =====================================================================
    """
    width, height = grid['size']
    layers = [(name, grid['layers'][name]) for name in CENSUS_LAYERS if name in grid['layers']]
    names = b''.join(struct.pack('<H', len(n)) + n.encode('ascii') for n in grid['names'])
    header = 40 + 36 * len(layers)
    cursor = header + len(names)
    cursor += -cursor % 4
    table, body = b'', b''
    for name, cells in layers:
        table += name.encode('ascii').ljust(32, b'\0') + struct.pack('<I', cursor + len(body))
        body += cells + b'\0' * (-len(cells) % 4)
    head = CENSUS_MAGIC + struct.pack('<If2f2I2I', VERSION, grid['cell'], *grid['origin'], width, height,
                                      len(layers), len(grid['names']))
    blob = head + table + names
    return blob + b'\0' * (-len(blob) % 4) + body


def read_census_grid(data: bytes) -> dict:
    if data[:8] != CENSUS_MAGIC:
        raise ValueError('not a living-world census grid')
    version, cell, x0, z0, width, height, layer_count, name_count = struct.unpack_from('<If2f2I2I', data, 8)
    if version != VERSION:
        raise ValueError(f'unsupported census grid version {version}')
    layers, cursor = {}, 40
    for _ in range(layer_count):
        name = data[cursor:cursor + 32].split(b'\0')[0].decode('ascii')
        offset = struct.unpack_from('<I', data, cursor + 32)[0]
        layers[name] = data[offset:offset + 2 * width * height]
        cursor += 36
    names = []
    for _ in range(name_count):
        length = struct.unpack_from('<H', data, cursor)[0]
        names.append(data[cursor + 2:cursor + 2 + length].decode('ascii'))
        cursor += 2 + length
    return {'cell': cell, 'origin': [x0, z0], 'size': [width, height], 'names': names, 'layers': layers}


def census_at(grid: dict, layer: str, x: float, z: float) -> str | None:
    """The census record a grid holds at world (x, z), or None (unpainted / outside)."""
    width, height = grid['size']
    i = int((x - grid['origin'][0]) // grid['cell'])
    j = int((z - grid['origin'][1]) // grid['cell'])
    if not (0 <= i < width and 0 <= j < height) or layer not in grid['layers']:
        return None
    value = struct.unpack_from('<H', grid['layers'][layer], 2 * (j * width + i))[0]
    return grid['names'][value - 1] if value else None


# ---------------------------------------------------------------- road network (0x00EB0013)

def parse_roads(blob: bytes) -> dict:
    """Header and segment table of one road network object.

    Layout (big-endian, offsets from the object start) [data, 82 objects on the disc]:
    +0x00 vec4 bbox min, +0x10 vec4 bbox max (x, y up, z, 1);
    +0x20 u32 intersection count, u32 lane-run count, u32 segment count (they differ where a segment
    has two lane runs in one tile: University cSim_150_250 has 3 runs for 2 segments; reading the run
    count as the segment count made a phantom segment before V0), u32 intersection table offset (stale
    pointer when the count is 0), u32 lane-run table offset, u32 segment table offset. Segments are 0x40 bytes: u64 id, u64 node A, u32 end A,
    pad, u64 node B, u32 end B, f32 length (m), f32 width A, f32 width B, f32 speed limit (m/s;
    14.17 = 51 km/h), u32 word_52 = lane count (equal to the lane runs' lane count on every run),
    u32 word_56 (2 or 3; meaning open), pad. Lanes run from node B to node A (A = destination).
    The junction and lane-run tables are decoded by ``living_world_roads`` (roads v2).
    """
    if len(blob) < 0x38:
        raise ValueError('road network object too short')
    lo = struct.unpack_from('>4f', blob, 0)
    hi = struct.unpack_from('>4f', blob, 16)
    intersections, runs, segments, intersection_at, nodes_at, segments_at = struct.unpack_from('>6I', blob, 0x20)
    if segments_at + 0x40 * segments > len(blob):
        raise ValueError('road segment table runs past the object end')
    out = []
    for i in range(segments):
        at = segments_at + 0x40 * i
        sid, node_a, end_a = struct.unpack_from('>QQI', blob, at)
        node_b, end_b, length, width_a, width_b, speed, word_52, word_56 = struct.unpack_from('>QIffffII', blob, at + 24)
        out.append({'id': f'{sid:016X}', 'node_a': f'{node_a:016X}', 'end_a': end_a,
                    'node_b': f'{node_b:016X}', 'end_b': end_b, 'length': _round(length),
                    'width_a': _round(width_a), 'width_b': _round(width_b), 'speed_limit': _round(speed),
                    'word_52': word_52, 'word_56': word_56})
    return {'bbox': [[_round(v) for v in lo[:3]], [_round(v) for v in hi[:3]]],
            'intersections': intersections, 'lane_runs': runs,
            'tables': {'intersections': intersection_at if intersections else None, 'nodes': nodes_at,
                       'segments': segments_at},
            'segments': out}


def write_pack(magic: bytes, tiles: list[tuple[int, str, bytes]]) -> bytes:
    """Container for verbatim RW objects (same layout as the skater path packs):
    +0 8 B magic, +8 u32 version (1), +12 u32 count, +16 count x 48 B (u64 asset id, u32 offset,
    u32 length, 32 B tile stream name), then the blobs, 16-byte aligned."""
    head = 16 + 48 * len(tiles)
    cursor = head + (-head % 16)
    table, body = b'', b''
    for asset_id, name, blob in tiles:
        table += struct.pack('<QII', asset_id, cursor + len(body), len(blob)) + name.encode('ascii')[:32].ljust(32, b'\0')
        body += blob + b'\0' * (-len(blob) % 16)
    data = magic + struct.pack('<II', 1, len(tiles)) + table
    return data + b'\0' * (cursor - len(data)) + body


def read_pack(data: bytes, magic: bytes) -> list[tuple[int, str, bytes]]:
    if data[:8] != magic:
        raise ValueError('wrong pack magic')
    version, count = struct.unpack_from('<II', data, 8)
    if version != 1:
        raise ValueError(f'unsupported pack version {version}')
    out = []
    for i in range(count):
        asset_id, offset, length = struct.unpack_from('<QII', data, 16 + 48 * i)
        name = data[32 + 48 * i:64 + 48 * i].split(b'\0')[0].decode('ascii')
        out.append((asset_id, name, data[offset:offset + length]))
    return out


# ---------------------------------------------------------------- waypoints (0x00EB001A)

def _text(blob: bytes, at: int) -> str:
    end = blob.index(b'\0', at)
    return blob[at:end].decode('latin-1')


def parse_waypoints(blob: bytes, type_names: dict[int, str]) -> list[dict]:
    """Waypoint groups of one object.

    Layout (big-endian) [data, 59 objects / 394 waypoints on the disc]: +0 u32 group count, u32
    waypoint count, u32 point count (3 per group), u32 group table offset (0x20), u32 waypoint table
    offset, u32 string offset. Group (0x60): vec4 centre, vec4 min, vec4 max, u64 name id, u64 GUID,
    u64 class id, u64 type id (vault hash of e.g. ``waypoint_vendingmachine``), u32 waypoint count,
    u32 first waypoint offset, u32 strings offset, u32 class-name offset. Waypoint (0x30): vec4
    position, vec4 facing, u32 index-or-flag, u32 group offset, u32 locator-name offset, u32
    class-name offset."""
    groups_count, waypoints_count, _, groups_at, waypoints_at, _ = struct.unpack_from('>6I', blob, 0)
    if groups_at + 0x60 * groups_count > len(blob):
        raise ValueError('waypoint group table runs past the object end')
    groups = []
    for g in range(groups_count):
        at = groups_at + 0x60 * g
        centre, lo, hi = (struct.unpack_from('>3f', blob, at + 16 * k) for k in range(3))
        name_id, guid, class_id, type_id = struct.unpack_from('>4Q', blob, at + 0x30)
        count, first, _, class_name = struct.unpack_from('>4I', blob, at + 0x50)
        points = []
        for w in range(count):
            wa = first + 0x30 * w
            pos = struct.unpack_from('>3f', blob, wa)
            facing = struct.unpack_from('>3f', blob, wa + 16)
            word, group_at, locator, _ = struct.unpack_from('>4I', blob, wa + 32)
            if group_at != at:
                raise ValueError(f'waypoint {w} points at group {group_at:#x}, not {at:#x}')
            points.append({'position': [_round(v) for v in pos], 'facing': [_round(v) for v in facing],
                           'word': word, 'locator': _text(blob, locator)})
        groups.append({'id': f'{name_id:016X}', 'guid': f'{guid:016X}', 'class_id': f'{class_id:016X}',
                       'type': type_names.get(type_id, f'Hash_{type_id:016X}'),
                       'box': {'centre': [_round(v) for v in centre], 'min': [_round(v) for v in lo],
                               'max': [_round(v) for v in hi]},
                       'class': _text(blob, class_name), 'waypoints': points})
    if sum(len(g['waypoints']) for g in groups) != waypoints_count:
        raise ValueError('waypoint count does not match the groups')
    return groups


# Waypoint types the TU3 image names (strings at 0x82066F00..; plugin anchors).
WAYPOINT_TYPES = ('waypoint_vendingmachine', 'waypoint_atm', 'waypoint_sit', 'waypoint_patrol', 'waypoint_spectator',
                  'waypoint_conversation', 'waypoint_lookat', 'waypoint_usetrashbin', 'waypoint_waterfountain',
                  'waypoint_newspaperbox', 'waypoint_actortrackerspectate')


# ---------------------------------------------------------------- recipes

class _Cursor:
    def __init__(self, data: bytes):
        self.data, self.at = data, 0

    def take(self, fmt: str):
        values = struct.unpack_from('>' + fmt, self.data, self.at)
        self.at += struct.calcsize('>' + fmt)
        return values if len(values) > 1 else values[0]

    def text(self) -> str:
        length = self.take('I')
        if length > 256 or self.at + length > len(self.data):
            raise ValueError(f'recipe string length {length} at {self.at:#x}')
        value = self.data[self.at:self.at + length].decode('ascii')
        self.at += length
        return value


def parse_recipe(data: bytes) -> dict:
    """A binary ``.recipe`` (big-endian, unaligned) [data, all ped / prop / vehicle recipes]:
    u32 version (7), string name, u32 word_a, u32 word_b, u32 word_c, u32 part count; per part:
    string slot (Rostral / Hair / Accessory / Equipment), u32 word, u64 slot id, u32 LOD count; per
    LOD: u64 id, u8 byte, u64 model arena id, u32 word = material instance count (1; 2 on reda_car's
    wheels), then per instance u32 material count (0 or 1), u64 material id and u32 texture count (both
    only if 1), per texture string channel + u64 texture id. Trailer: zero padding to a
    4-byte boundary, zero or one u32 0 (meaning open), then u32 = its own offset (file size - 4). Strings are u32 length + bytes
    (no NUL)."""
    c = _Cursor(data)
    version = c.take('I')
    if version != RECIPE_VERSION:
        raise ValueError(f'unsupported recipe version {version}')
    name = c.text()
    words = [c.take('I') for _ in range(3)]
    parts = []
    for _ in range(c.take('I')):
        slot = c.text()
        word = c.take('I')
        slot_id = c.take('Q')
        lods = []
        for _ in range(c.take('I')):
            lod_id, byte, model, lod_word = c.take('QBQI')
            if not 1 <= lod_word <= 8:
                raise ValueError(f'{name}/{slot}: {lod_word} material instances in one LOD')
            instances = []
            for _ in range(lod_word):  # word = material instance count (2 on reda_car's wheels) [data]
                materials = c.take('I')
                if materials > 1:
                    raise ValueError(f'{name}/{slot}: {materials} materials in one instance')
                material = c.take('Q') if materials else None
                textures = {}
                for _ in range(c.take('I') if materials else 0):  # no texture list without a material
                    channel = c.text()
                    textures[channel] = f'{c.take("Q"):016x}'
                instances.append({'material': f'{material:016x}' if material is not None else None,
                                  'textures': textures})
            lod = {'id': f'{lod_id:016x}', 'byte': byte, 'model': f'{model:016x}', 'word': lod_word, **instances[0]}
            if lod_word > 1:
                lod['instances'] = instances
            lods.append(lod)
        parts.append({'slot': slot, 'word': word, 'id': f'{slot_id:016x}', 'lods': lods})
    padding = data[c.at:c.at + (-c.at % 4)]
    c.at += len(padding)
    zero_words = 0
    while c.at + 8 <= len(data) and struct.unpack_from('>I', data, c.at)[0] == 0:
        c.at += 4
        zero_words += 1
    end = c.at
    size = c.take('I') if c.at + 4 <= len(data) else None
    if padding.strip(b'\0') or size != end or c.at != len(data):
        raise ValueError(f'{name}: recipe trailer {size} at {end:#x} of {len(data)}')
    return {'version': version, 'name': name, 'words': words, 'parts': parts, 'trailer_zero_words': zero_words}


def recipe_shaders(xml: bytes) -> dict:
    """Material id (16 hex digits, as ``parse_recipe`` writes them) -> material type from a recipe's
    XML twin (``<mat id="0x..." type="pedestrian_high_stamp">``). The type picks the retail shader:
    ``pedestrian_high_stamp`` / ``pedestrian_low`` are the ped body shaders that recolour the mask
    texels with the model's tints, ``marquee_hair`` / ``marquee_cloth`` / ``cac_alpha`` do not [data]."""
    import re
    text = xml.decode('utf-8', 'replace')
    return {m.group(1).lower().rjust(16, '0'): m.group(2)
            for m in re.finditer(r'<mat\s+id="0x([0-9A-Fa-f]+)"\s+type="([^"]+)"', text)}


def model_manifest(game_root: Path) -> dict:
    """models.json: every binary recipe under ``recipe/livingworld/`` with its arenas checked."""
    from tools.owned_game.big import BigArchive
    big = BigArchive(game_root/'data/content/livingworld.big')
    paths = {e.path.lower(): e for e in big.entries}
    recipes, errors = {}, []
    for entry in sorted(big.entries, key=lambda e: e.path):
        path = entry.path.lower()
        if not (path.startswith('data/content/recipe/livingworld/') and path.endswith('.recipe')):
            continue
        try:
            recipe = parse_recipe(big.read(entry))
        except (ValueError, struct.error, UnicodeDecodeError) as error:
            errors.append(f'{entry.path}: {error}')
            continue
        missing = []
        for part in recipe['parts']:
            for lod in part['lods']:
                lod['arena'] = f"data/content/livingworld/model/{recipe['name']}/{part['slot']}/0x{lod['model']}.rx2"
                if lod['arena'].lower() not in paths:
                    missing.append(lod['arena'])
                for tid in lod['textures'].values():
                    texture = f'data/content/livingworld/texture/0x{tid}.rx2'
                    if texture not in paths:
                        missing.append(texture)
        stem = Path(entry.path).stem
        xml = paths.get(f'data/content/recipe/livingworld/{stem}.xml')
        recipes[stem] = {**recipe, 'kind': 'prop' if stem.startswith('zprop_') else 'ped',
                         'xml': xml is not None,
                         'shaders': recipe_shaders(big.read(xml)) if xml is not None else {},
                         'missing': sorted(set(missing))}
    return {'version': VERSION, 'archive': 'data/content/livingworld.big', 'recipes': recipes, 'errors': errors,
            'peds': sorted(k for k, r in recipes.items() if r['kind'] == 'ped'),
            'props': sorted(k for k, r in recipes.items() if r['kind'] == 'prop')}


# ---------------------------------------------------------------- validation

def validate(private: Path) -> list[str]:
    """Problems with an exported living world (empty = good): the tables load, every census record
    the grids paint resolves through its category group to entities whose model recipe exists."""
    root = Path(private)/'living_world'
    problems = []
    try:
        doc = json.loads((root/'tables.json').read_text(encoding='utf-8'))
        models = json.loads((root/'models.json').read_text(encoding='utf-8'))
        census_index = json.loads((root/'census.json').read_text(encoding='utf-8'))
    except (OSError, ValueError) as error:
        return [f'living world tables do not load: {error}']
    if doc.get('version') != VERSION:
        problems.append(f"tables.json version {doc.get('version')}")
    recipes = models.get('recipes', {})
    painted = {'pedestrians'}  # the generic record behind every district record
    for district, info in census_index.get('districts', {}).items():
        try:
            grid = read_census_grid((root/info['file']).read_bytes())
        except (OSError, ValueError) as error:
            problems.append(f'{district}: census grid: {error}')
            continue
        if not grid['names']:
            problems.append(f'{district}: census grid is empty')
        painted.update(info.get('npc_records', []))
    census = doc['classes'].get('livingworld_census', {})
    npc_records = sorted(painted)
    for record in npc_records:
        if record not in census:
            problems.append(f'census record {record} is not in livingworld_census')
            continue
        chain = census_chain(doc, record)
        if not chain['categories']:
            problems.append(f'census {record}: no categories')
        for category in chain['categories']:
            if not category['entities']:
                problems.append(f"census {record}: category {category['category']} has no entities")
            for entity in category['entities']:
                if not entity['recipes']:
                    problems.append(f"census {record}: entity {entity['entity']} has no model recipe")
                for recipe in entity['recipes']:
                    if recipe not in recipes:
                        problems.append(f"census {record}: entity {entity['entity']} recipe {recipe} is not in livingworld.big")
                    elif recipes[recipe]['missing']:
                        problems.append(f"recipe {recipe}: missing {recipes[recipe]['missing'][:3]}")
                    elif 'glb' in recipes[recipe] and not (recipes[recipe]['glb'].get('status') == 'ready'
                                                         and (root/recipes[recipe]['glb']['file']).is_file()):
                        problems.append(f"recipe {recipe}: model not built ({recipes[recipe]['glb'].get('error')})")
    return sorted(set(problems))


# ---------------------------------------------------------------- entry points

def _get(ctx, name, default=None):
    if isinstance(ctx, dict):
        return ctx.get(name, default)
    return getattr(ctx, name, default)


def _paths(ctx):
    game_root = Path(_get(ctx, 'game_root'))
    private = _get(ctx, 'private')
    private = Path(private) if private is not None else Path(_get(ctx, 'stage'))/'assets/private'
    return game_root, private, Path(_get(ctx, 'work')), _get(ctx, 'report') or (lambda text: None)


def _collections(ctx, game_root: Path, work: Path) -> list[dict]:
    collections = _get(ctx, 'collections')
    if collections is None:
        from .living_world_skaters import convert_database
        collections = convert_database(game_root, work/'living_world_db')['collections']
        if isinstance(ctx, dict):
            ctx['collections'] = collections  # the skater half reuses the conversion
    return collections


def export(ctx) -> dict:
    """Export the pedestrian half (tables, census grids, roads, waypoints, navmesh inventory,
    model manifest). ``ctx`` as in ``living_world_skaters.export``. Returns a report dict."""
    from .audio_formats import REGION_PROCESSOR, region_layers
    from .vlt import hash64
    game_root, private, work, report = _paths(ctx)
    output = private/'living_world'
    output.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    warnings = []

    report('Exporting living-world tables')
    collections = _collections(ctx, game_root, work)
    doc = tables(collections)
    from .living_world_anim import collections_bin, resolve_anim_names
    resolve_anim_names(doc, collections_bin(game_root, work))  # clip names for the ped remaps (M2)
    (output/'tables.json').write_text(json.dumps(doc, indent=1), encoding='utf-8')

    report('Reading living-world census layers, roads and waypoints')
    record_names = census_record_names(collections)
    type_names = {hash64(n): n for n in WAYPOINT_TYPES}
    layers: dict[str, dict[str, list]] = {}
    roads: dict[str, list] = {}
    waypoints: dict[str, list] = {}
    navmesh: dict[str, dict] = {}
    from . import living_world_navmesh
    nav_graphs: dict[str, list] = {}
    for district, tile, asset_id, processor, data in district_assets(game_root, work):
        if processor == REGION_PROCESSOR:
            for layer in region_layers(data):
                if layer['layer'] in CENSUS_LAYERS:
                    layers.setdefault(district, {}).setdefault(layer['layer'], []).append(layer)
        for blob in arena_objects(data, ROADDATA):
            roads.setdefault(district, []).append((asset_id, tile, blob))
        for blob in arena_objects(data, WAYPOINTDATA):
            for group in parse_waypoints(blob, type_names):
                waypoints.setdefault(district, []).append({'tile': tile, **group})
        for blob in arena_objects(data, NAVPOWERDATA):
            info = navmesh.setdefault(district, {'objects': 0, 'bytes': 0, 'tiles': set()})
            info['objects'] += 1
            info['bytes'] += len(blob)
            info['tiles'].add(tile)
            graph = living_world_navmesh.parse_graph(blob)
            if graph is not None:
                nav_graphs.setdefault(district, []).append((tile, asset_id, graph))

    report('Writing living-world census grids')
    census_index = {'version': VERSION, 'cell': CENSUS_CELL, 'layers': list(CENSUS_LAYERS), 'districts': {}}
    for district, district_layers in sorted(layers.items()):
        grid = census_grid(district_layers, record_names)
        file = f'{district}.census.bin'
        (output/file).write_bytes(write_census_grid(grid))
        npc = sorted({record_names.get(k, f'Hash_{k:016X}') for l in district_layers.get('livingworld_npc_census', [])
                      for k in l['keys']})
        vehicle = sorted({record_names.get(k, f'Hash_{k:016X}') for l in district_layers.get('livingworld_vehicle_census', [])
                          for k in l['keys']})
        for name in npc + vehicle:
            if name.startswith('Hash_'):
                warnings.append(f'{district}: census key {name} is not a livingworld_census record')
        lost = sorted(set(npc + vehicle) - set(grid['names']))
        if lost:
            warnings.append(f'{district}: census records in the layer key tables but in no grid cell: {lost}')
        census_index['districts'][district] = {
            'file': file, 'origin': grid['origin'], 'size': grid['size'], 'names': grid['names'],
            'layers': {name: len(district_layers.get(name, [])) for name in CENSUS_LAYERS},
            'npc_records': npc, 'vehicle_records': vehicle}
    (output/'census.json').write_text(json.dumps(census_index, indent=1), encoding='utf-8')

    report('Writing the road network and waypoints')
    from . import living_world_roads
    road_doc = {'version': ROADS_VERSION, 'graph': 'roads.bin', 'raw': 'roads_raw.bin', 'districts': {}}
    pack, graphs = [], {}
    for district, items in sorted(roads.items()):
        items.sort(key=lambda e: (e[1], e[0]))
        objects, tiles = [], {}
        for asset_id, tile, blob in items:
            parsed = parse_roads(blob)
            objects.append({'tile': tile, 'asset_id': f'{asset_id:016X}', 'bbox': parsed['bbox'],
                            'intersections': parsed['intersections'], 'segments': [s['id'] for s in parsed['segments']]})
            for segment in parsed['segments']:
                tiles.setdefault(segment['id'], []).append(tile)
            pack.append((asset_id, f'{district[:3]}:{tile}', blob))
        graph = graphs[district] = living_world_roads.build([blob for _, _, blob in items])
        warnings += [f'{district}: road network: {problem}' for problem in graph['problems']]
        view = living_world_roads.summary(graph)
        for sid, segment in view['segments'].items():
            segment['tiles'] = tiles.get(sid, [])
        nodes = sorted({s['node_a'] for s in view['segments'].values()} | {s['node_b'] for s in view['segments'].values()})
        road_doc['districts'][district] = {'objects': objects, 'segments': view['segments'], 'nodes': nodes,
                                           'junctions': view['junctions'],
                                           'intersections': sum(o['intersections'] for o in objects)}
    (output/'roads.bin').write_bytes(living_world_roads.write_graph(graphs))
    (output/'roads_raw.bin').write_bytes(write_pack(ROADS_MAGIC, pack))
    (output/'roads.json').write_text(json.dumps(road_doc, indent=1), encoding='utf-8')
    (output/'waypoints.json').write_text(json.dumps({'version': VERSION, 'districts': waypoints}, indent=1),
                                         encoding='utf-8')
    meshes = {d: living_world_navmesh.merge_district([g for _, _, g in sorted(items, key=lambda e: (e[0], e[1]))])
              for d, items in sorted(nav_graphs.items())}
    (output/'navmesh.bin').write_bytes(living_world_navmesh.write_navmesh(meshes))
    (output/'navmesh.json').write_text(json.dumps({'version': VERSION, 'decoded': True, 'file': 'navmesh.bin', 'districts': {
        d: {**i, 'tiles': len(i['tiles']), **(living_world_navmesh.summary(meshes[d]) if d in meshes else {})}
        for d, i in sorted(navmesh.items())}}, indent=1), encoding='utf-8')

    report('Reading pedestrian recipes')
    models = model_manifest(game_root)
    warnings += models['errors']
    report('Building pedestrian and hand-prop models')
    from .living_world_models import export_models
    built = export_models(game_root, models, output/'models', work/'living_world_models', report)
    for name, result in built.items():
        models['recipes'][name]['glb'] = result
        if result['status'] != 'ready':
            warnings.append(f"model {name}: {result['error']}")
    (output/'models.json').write_text(json.dumps(models, indent=1), encoding='utf-8')

    from . import living_world_vehicles
    vehicle_records = sorted({r for i in census_index['districts'].values() for r in i['vehicle_records']})
    vehicles = living_world_vehicles.export(game_root, output, work, doc, vehicle_records, report)
    warnings += vehicles.pop('warnings')

    problems = validate(private)
    if problems:
        raise ValueError('living world data does not resolve: ' + '; '.join(problems[:5]))
    return {
        'version': VERSION,
        'classes': {cls: len(rows) for cls, rows in doc['classes'].items()},
        'census': {d: {'records': len(i['names']), 'size': i['size']} for d, i in census_index['districts'].items()},
        'road_segments': {d: len(i['segments']) for d, i in road_doc['districts'].items()},
        'road_junctions': {d: len(i['junctions']) for d, i in road_doc['districts'].items()},
        'road_connectors': {d: sum(len(j['connectors']) for j in i['junctions'].values())
                            for d, i in road_doc['districts'].items()},
        'vehicles': vehicles,
        'waypoint_groups': {d: len(g) for d, g in waypoints.items()},
        'navmesh_objects': {d: i['objects'] for d, i in navmesh.items()},
        'recipes': {'peds': len(models['peds']), 'props': len(models['props'])},
        'models_ready': sum(1 for r in built.values() if r['status'] == 'ready'),
        'warnings': warnings,
    }


def export_group(ctx) -> dict:
    """The ``livingworld`` setup group: the ped half, then the NPC skater half
    (``living_world_skaters.export``), sharing one vault conversion. Raises on failure; the caller
    records the group as unavailable and the world runs empty."""
    from . import living_world_skaters
    game_root, private, work, report = _paths(ctx)
    context = {'game_root': game_root, 'private': private, 'work': work, 'report': report,
               'collections': _get(ctx, 'collections')}
    peds = export(context)
    skaters = living_world_skaters.export(context)
    output = private/'living_world'
    files = sorted(p for p in output.rglob('*') if p.is_file() and p.name != 'export.json')
    summary = {'version': VERSION, 'peds': peds, 'skaters': {k: v for k, v in skaters.items() if k != 'sha256'},
               'sha256': {p.relative_to(private).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}}
    (output/'export.json').write_text(json.dumps(summary, indent=1), encoding='utf-8')
    return summary


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description='Export the living world from an extracted disc.')
    parser.add_argument('--game', type=Path, required=True)
    parser.add_argument('--private', type=Path, required=True, help='output assets/private folder')
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--peds-only', action='store_true')
    args = parser.parse_args()
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
    ctx = {'game_root': args.game, 'private': args.private, 'work': args.work, 'report': print}
    result = export(ctx) if args.peds_only else export_group(ctx)
    print(json.dumps({k: v for k, v in result.items() if k != 'sha256'}, indent=1))
