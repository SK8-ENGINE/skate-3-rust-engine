"""Export the original front-end menu movies for the engine's APT player.

Setup runs this inside the HUD stage (prepare_runtime_huds.py). It reads the
owned disc's fedata.big / fetexture.big / miscboot.big (and fedynamic.big for the
named runtime images), starts from the menu
screens in MENU_ROOTS and follows every cross-movie import (retail load queue
sub_82E7E8C0 requests each import source by name before linking), so the export
holds the menu movies plus every library movie they import.

Output (assets/private/menu-movies/, never committed):
  manifest.json            format, roots, movie key -> file, shared language/fonts, file hashes
  shared.json              language table, font definitions and native font layout (shared by all movies)
  movies/<key>.json        one movie in the HUD player format (characters, actions, shapes)
                           plus imports / exports / character ids for apt_imports.rs
  <cache paths>            the RGBA payloads the shapes and fonts reference
  manifest images          named runtime images (request key -> RGBA payload, size) the movies load by name
                           through Game.ImageRenderer_Load (the challenge map pictures)

Movie keys are the retail import paths relative to data/fe/ without extension
(apt_imports.rs library_key), e.g. source/screens/main/core_menu.
No game content is embedded in this tool: only screen names.
"""
from pathlib import Path
import argparse
import hashlib
import json
import shutil

from vendor.skate3_ui.project import extract_project
from vendor.skate3_ui.scene_graph import AssetCache, SceneFlattener
from vendor.skate3_ui.actions import Actions

FORMAT = 'skate3-menu-movies'
VERSION = 14  # 2: placement clip actions (Milestone 3d); 3: unit bitmap ids (Milestone 4); 4: screens/main root (Milestone 6); 5: main.apt's layer movies; 6: the challenge map; 7: named runtime images (challenge map pictures); 8: the Skate Profile (FE 61); 9: Extras (FE 18); 10: the text viewer (FE 3); 11: the replay editor and help (FE 12 / 13); 12: every DLC pack's text tables (Challenge Map DLC location names); 13: the credits (FE 25) and their lines file; 14: SpawnSkater (FE 62)
# Menu screens the engine's retail menus start from (pause menu, Game Settings,
# Freeskate options, the screen manager and its helpers). Library movies are
# found through imports, not listed.
# APT font name -> paired native font file (825D6B68; apt_text.rs RETAIL_FONT_PAIRS).
FONT_PAIRS = {'Futura Shadow': 'futuraheavy'}

MENU_ROOTS = (
    # The front end's _level0 (defines Register*Enum, which the C++ calls through _level0._root).
    'source/screens/main',
    # The layers main.apt loads (gChyron, gSubtitle, gFadeBackground, autosave, fmv popup).
    'source/screens/main/chyron',
    'source/screens/main/subtitle',
    'source/screens/main/fmv_popup',
    'source/controls/fadebackground',
    'source/controls/autosave',
    'source/screens/main/FE_root',
    'source/screens/main/core_menu',
    'source/screens/main/dimmer',
    'source/screens/main/freeskate_options',
    'source/screens/options/options',
    # Game Settings > Video > Video Calibration (template 91 VideoCalibration, FE screen 51).
    'source/screens/options/videocal',
    # The pause menu's Learn > Trick Guide (template 29 TrickTutorial).
    'source/screens/tricks/trickguide',
    # The pause menu's ChallengeMap / OnlineChallengeMap (template 68 ChallengeMap, FE screen 17, host object
    # FEChallengeMap).
    'source/screens/map/challengemap',
    # The Free Play pause row Skate Profile (template 106 OfflineProfile, FE screen 61, host objects Progression and
    # Stats; research b308).
    'source/screens/profile/offline_skateProfile',
    # The Free Play pause row Extras (template 15 Extras, FE screen 18; research b320).
    'source/screens/main/extras',
    # The text viewer the Extras Licenses row opens (template 28 ToSScreen, FE screen 3; research b322).
    'source/screens/nucleus/nucleus_tos',
    # The Free Play pause rows Free Play / Call Skater (template 107 SpawnSkater, FE screen 62, host object
    # SkaterSpawner; research b543).
    'source/screens/cas/spawnskater',
    # The instant replay / replay editor screen (template 72 Replay2, FE screen 12) and its help (template 73
    # ReplayHelp, FE screen 13); research b385 / b390 / b391.
    'source/screens/replay2/replay_editor2',
    'source/screens/replay2/replay_help',
    # The action overlay (template 59 SignUpHUD): the teleporter prompt (state 2, research b421 / b426). Optional for
    # the engine: a set without it keeps the teleporter's text line, so the set version stays.
    'source/screens/hud2/actionoverlay2',
    # The loading screen (template 131 TipsLoader, FE screen 52, host object TipsLoader; research b581). Optional for
    # the engine like the action overlay: a set without it keeps the progress overlay, so the set version stays.
    'source/screens/main/tiploader',
    'source/screens/main3dhud',
    'source/helper/screenmanager',
    'source/helper/screentransitionmanager',
    'source/controls/tabs',
    'source/controls/scrollbar',
    'source/screens/popup/sk8popup',
    'source/screens/popup/small_popup',
)
# Movies only in fedynamic.big, extracted into their own cache (prefix-limited, so no movie above changes archive):
# the Extras Credits row's screen (template 30 Credits, FE screen 25, host object FECredits; research b489).
DYNAMIC_ROOTS = ('source/screens/extras/credits',)
DYNAMIC_PREFIXES = ('data/fe/source/screens/extras/',)
# The credit lines the credits screen reads (cCreditsManager, `data\credits\credits.txt` in miscload.big), copied
# into the set as `credits.txt` (retail_menu_stage.rs CREDITS_FILE).
CREDITS_ARCHIVE = 'miscload.big'
CREDITS_ENTRY = 'data/credits/credits.txt'
# Movies from the player's own DLC packages (optional): the Skate.Create Upgrade Pack (product CCF0007) carries the
# DLC replay editor (template 71 Replay3, `replay2/replay_editor3.swf`, with the look channels) and its help
# (template 74 ReplayHelp2); research b410 / b411. Exported only when the package is there.
DLC_ROOTS = (
    'source/screens/replay2/replay_editor3',
    'source/screens/replay2/replay_help2',
)
MAX_MOVIES = 256  # apt_imports.rs MAX_MOVIES
# Runtime images the movies load by name (Game.ImageRenderer_Load, research b257 / b258): only in fedynamic.big.
IMAGE_PREFIXES_DYNAMIC = ('data/fe/source/images/map/',)
# Texture packs whose textures the engine draws by texture name (retail looks them up as FNV-1 64 of "<name>.Texture"
# in the texture manager, research b263): the Challenge Map marker icons. Key: texture/<name>.
TEXTURE_PACKS_DYNAMIC = ('data/fe/source/images/icons/minimap',)
# Named images already in the main cache (fetexture.big).
IMAGE_NAMES_CORE = ('data/fe/source/screens/map/cursorchallengemap',)


def library_key(file):
    """Same rule as apt_imports.rs library_key."""
    path = file.replace('\\', '/').lstrip('/')
    if path.lower().startswith('data/fe/'):
        path = path[len('data/fe/'):]
    return path[:-4] if path.endswith('.apt') else path


def _digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _contained(root, relative):
    path = (root / relative).resolve()
    if not path.is_relative_to(root.resolve()) or path == root.resolve():
        raise ValueError(f'Menu movie path escapes its folder: {relative}')
    return path


# The world records' map picture (`Sk8::FE::tMapInfo` word 0 = the name's offset in skatercollections.bin, research
# b257 / b259): resolved here, in the menu stage, so the vault converter (core / living world / customiser fingerprints)
# stays unchanged.
WORLD_MAP_INFO = 'Hash_20BBC594CD2FAEF8'


def dlc_world_maps(game, dlc, work):
    """The DLC packs' world records with their own tMapInfo, each pointer resolved against its own vault's `.bin`
    (research b417: against the disc's `.bin` the Maloof NYC pointer reads another name), and the BIGs holding their
    pictures (`data/fe/source/images/map/`)."""
    from vendor.skate3_ui.big import BigArchive
    from asset_pipeline import dlc as packages
    from vendor.skate3_ui.project import find_big_directory
    images = lambda paths: any(p.lower().startswith(IMAGE_PREFIXES_DYNAMIC) for p in paths)  # noqa: E731
    vaulted = lambda paths: any(p.lower().startswith('data/db/') and p.lower().endswith('.vlt') for p in paths)  # noqa: E731
    bigs = packages.extract_bigs(dlc, work / 'bigs', wanted=lambda paths: images(paths) or vaulted(paths))
    schema = work / 'schema'
    database = BigArchive(find_big_directory(game) / 'db.big')
    schema.mkdir(parents=True, exist_ok=True)
    for entry in database.entries:
        name = Path(entry.path).name.lower()
        if name in ('skaterschema.bin', 'skaterschema.vlt'):
            (schema / name).write_bytes(database.read(entry))
    names = (Path(__file__).resolve().parent / 'asset_pipeline/names.txt').read_text(encoding='utf-8').splitlines()
    out = {}
    for v in packages.vaults(bigs, schema / 'skaterschema', names, work / 'vaults'):
        for row in v['converted']['collections']:
            field = row.get('fields', {}).get(WORLD_MAP_INFO)
            if field and len(field.get('data', '')) >= 8:
                pointer = int(field['data'][:8], 16)
                end = v['binary'].find(bytes(1), pointer) if pointer else -1
                if pointer and 0 < pointer < len(v['binary']) and end > pointer:
                    out.setdefault(row['key'], bytes(v['binary'][pointer:end]).decode('ascii'))
    return out, [big for big in bigs if images([e.path.replace('\\', '/') for e in BigArchive(big).entries])]


def world_maps(game, collections, work):
    """{world record key: map picture name} for every world record that sets its own tMapInfo (children inherit
    through the collections' parent chain; the engine walks it)."""
    from vendor.skate3_ui.big import BigArchive
    from asset_pipeline.vlt import vault
    from vendor.skate3_ui.project import find_big_directory
    database = BigArchive(find_big_directory(game) / 'db.big')
    work.mkdir(parents=True, exist_ok=True)
    for entry in database.entries:
        name = Path(entry.path).name.lower()
        if name in ('skatercollections.bin', 'skatercollections.vlt'):
            (work / name).write_bytes(database.read(entry))
    _, binary, _ = vault(work / 'skatercollections')
    rows = json.loads(Path(collections).read_text(encoding='utf-8'))['collections']
    out = {}
    for row in rows:
        field = row.get('fields', {}).get(WORLD_MAP_INFO)
        if not field or len(field.get('data', '')) < 8:
            continue
        pointer = int(field['data'][:8], 16)
        end = binary.find(b'\0', pointer) if pointer else -1
        if pointer and 0 < pointer < len(binary) and end > pointer:
            out[row['key']] = bytes(binary[pointer:end]).decode('ascii')
    return out


def pack_textures(cache_root, runtime, packs):
    """{'texture/<name>': {'rgba', 'width', 'height'}} for every decoded texture of the bundles named in `packs`;
    copies the RGBA payloads into `runtime`."""
    manifest = json.loads((cache_root / 'manifest.json').read_text(encoding='utf-8'))
    out = {}
    for record in manifest['bundles']:
        name = record['name'].replace('\\', '/').lower()
        if name not in packs or 'textures' not in record:
            continue
        folder = Path(record['textures']['manifest']).parent
        for texture in json.loads(_contained(cache_root, record['textures']['manifest']).read_text(encoding='utf-8'))['textures']:
            if 'rgba_file' not in texture:
                continue
            relative = (folder / texture['rgba_file']).as_posix()
            target = _contained(runtime, relative)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(_contained(cache_root, relative), target)
            key = 'texture/' + texture['name'].lower().removesuffix('.texture')
            out[key] = {'rgba': relative, 'width': texture['width'], 'height': texture['height']}
    return out


def named_images(cache_root, runtime, prefixes):
    """{request key: {'rgba', 'width', 'height'}} for every single-texture .rx2 bundle under `prefixes`; copies the
    RGBA payloads into `runtime`. Key = the retail request path (ImageRenderer_Load 'fe\\source\\images\\map\\map1024')
    with '/', lower case, without 'data/' and '.rx2'."""
    manifest = json.loads((cache_root / 'manifest.json').read_text(encoding='utf-8'))
    out = {}
    for record in manifest['bundles']:
        name = record['name'].replace('\\', '/').lower()
        if not any(name.startswith(p) for p in prefixes) or 'textures' not in record:
            continue
        listed = json.loads(_contained(cache_root, record['textures']['manifest']).read_text(encoding='utf-8'))['textures']
        if len(listed) != 1 or 'rgba_file' not in listed[0]:
            raise ValueError(f'{name}: expected one decoded texture')
        texture = listed[0]
        relative = (Path(record['textures']['manifest']).parent / texture['rgba_file']).as_posix()
        target = _contained(runtime, relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(_contained(cache_root, relative), target)
        out[name.removeprefix('data/')] = {'rgba': relative, 'width': texture['width'], 'height': texture['height']}
    return out


PLACE_HAS_CLIP_ACTIONS = 0x80
MAX_CLIP_ACTIONS = 64  # apt_display.rs MAX_CLIP_ACTIONS


def clip_actions(apt, pointer):
    """Clip-event table of a placement (doc 31, Milestone 3d; retail sub_82E7B340 passes
    placement+60, sub_82E5B158 walks it): u32 count, u32 events; 12-byte events
    {u32 flags, u32 key_code, u32 actions}. Big-endian, offsets relative to the .apt."""
    def u32(offset):
        if offset < 0 or offset + 4 > len(apt):
            raise ValueError(f'clip action table outside the movie at {offset:#x}')
        return int.from_bytes(apt[offset:offset + 4], 'big')
    count, events = u32(pointer), u32(pointer + 4)
    if count > MAX_CLIP_ACTIONS:
        raise ValueError(f'{count} clip actions at {pointer:#x}')
    out = []
    for i in range(count):
        base = events + i * 12
        out.append({'flags': u32(base), 'key_code': u32(base + 4), 'actions_offset': u32(base + 8)})
    return out


def _with_bitmap(bundle, primitive):
    """Adds the bitmap character a textured GEO unit samples (unit +0x14, the id the flattener
    resolved the texture through), so the engine can name the bitmap by a stable id
    (movie key + bitmap character id) and a mod can replace it (doc 31, Milestone 4)."""
    if primitive.get('texture'):
        unit = bundle['shapes'][primitive['character_id']]['units'][primitive['unit']]
        primitive['bitmap'] = unit['texture_id']
    return primitive


def export_movie(cache, raw, key):
    """One movie in the HUD player JSON format plus its import tables."""
    name = 'data/fe/' + key
    bundle = cache.load_bundle(name)
    apt_path = raw / (name + '.apt')
    const_path = apt_path.with_suffix('.const')
    apt_bytes = apt_path.read_bytes()
    actions = Actions(apt_bytes, const_path.read_bytes())
    blocks, shapes, unresolved_shapes, families = {}, {}, {}, set()
    characters = list(bundle['characters'].values())
    for c in characters:
        for f in c.get('frames', []):
            for control in f['controls']:
                if control['type_name'] in ('do_action', 'do_init_action'):
                    offset = control.get('actions_offset', 0)
                    if offset:
                        blocks[str(offset)] = actions.stream(offset)
                elif control['type_name'] in ('place_object2', 'place_object3') \
                        and control.get('flags', 0) & PLACE_HAS_CLIP_ACTIONS and control.get('actions_offset', 0):
                    control['clip_actions'] = clip_actions(apt_bytes, control['actions_offset'])
                    for event in control['clip_actions']:
                        blocks[str(event['actions_offset'])] = actions.stream(event['actions_offset'])
        if c['type_name'] == 'shape':
            try:
                scene = SceneFlattener(cache, lambda *_: {}).flatten(name, c['id'])
            except Exception as error:  # noqa: BLE001 - drawing is a later milestone
                unresolved_shapes[str(c['id'])] = str(error)
                continue
            if scene['unresolved']:
                unresolved_shapes[str(c['id'])] = scene['unresolved']
            else:
                shapes[str(c['id'])] = [_with_bitmap(bundle, p) for p in scene['primitives']]
        elif c['type_name'] == 'font':
            families.add(c['font']['name'])
    apt = bundle['apt']
    return {
        'format': FORMAT, 'version': VERSION, 'movie': key,
        'source': {'bundle': name,
                   'apt_sha256': hashlib.sha256(apt_path.read_bytes()).hexdigest(),
                   'const_sha256': hashlib.sha256(const_path.read_bytes()).hexdigest()},
        'characters': characters,
        'shapes': shapes, 'unresolved_shapes': unresolved_shapes, 'actions': blocks,
        'imports': [{k: i[k] for k in ('file', 'name', 'character_id')} for i in apt.get('imports', [])],
        'exports': [{k: e[k] for k in ('name', 'character_id')} for e in apt.get('exports', [])],
    }, families


def dlc_archives(dlc, work):
    """The DLC BIGs holding the DLC movies we export (copied out of the player's packages to `work`)."""
    from asset_pipeline import dlc as packages
    from vendor.skate3_ui.big import BigArchive
    # Only archives carrying a movie we export: other packs' front-end files must not replace the disc's.
    wanted = {('data/fe/' + root + '.apt').lower() for root in DLC_ROOTS}
    menus = lambda paths: any(p.lower() in wanted for p in paths)  # noqa: E731
    return [(path, BigArchive(path)) for path in packages.extract_bigs(dlc, work, wanted=menus)]


def dlc_text_archives(dlc, work):
    """Every DLC BIG with front-end language tables, for their text only (the map packs name their locations and
    worlds there, e.g. ID_WORLD_MALOOFMONEYCUP_NYC; laptop session 2026-10-21: the Challenge Map showed the raw keys)."""
    from asset_pipeline import dlc as packages
    from vendor.skate3_ui.big import BigArchive
    text = lambda paths: any(p.lower().startswith('data/fe/languages/') for p in paths)  # noqa: E731
    return [BigArchive(path) for path in packages.extract_bigs(dlc, work, wanted=text)]


def prepare(game, output, collections, roots=MENU_ROOTS, dlc=None):
    """Builds the runtime set in `output` (a work folder) and returns its manifest. `dlc`: the player's DLC
    package folder (optional); its front-end movies and text join the disc's."""
    from prepare_hud import font_mapping
    from asset_pipeline import dlc as packages
    cache_root = output / 'cache'
    extras = dlc_archives(dlc, output / 'dlc') if dlc else []
    extract_project(game, cache_root, prefixes=('data/fe/source/',), update=True,
                    extra_archives=tuple(path for path, _ in extras))
    roots = tuple(roots) + tuple(r for r in DLC_ROOTS
                                 if (cache_root / 'raw' / ('data/fe/' + r + '.apt')).is_file() and r not in roots)
    cache = AssetCache(cache_root)
    mappings = font_mapping(collections, cache)
    # fedynamic-only movies: their own narrow cache; without fedynamic.big they are reported missing.
    dynamic_root = output / 'cache-movies-dynamic'
    try:
        extract_project(game, dynamic_root, include_dynamic=True, prefixes=DYNAMIC_PREFIXES, update=True)
        dynamic = AssetCache(dynamic_root)
    except FileNotFoundError:
        dynamic = None
    roots = roots + tuple(r for r in DYNAMIC_ROOTS if r not in roots)
    runtime = output / 'runtime'
    if runtime.exists():
        shutil.rmtree(runtime)
    (runtime / 'movies').mkdir(parents=True)
    queue, movies, families, missing = list(roots), {}, set(), []
    # The cache each exported movie came from (its payloads are copied from there).
    sources = {}
    while queue:
        key = library_key(queue.pop(0))
        if key in movies or key in missing:
            continue
        if len(movies) >= MAX_MOVIES:
            raise ValueError('Too many menu movies')
        apt = 'data/fe/' + key + '.apt'
        if (cache_root / 'raw' / apt).is_file():
            source, source_cache = cache_root, cache
        elif dynamic is not None and (dynamic_root / 'raw' / apt).is_file():
            source, source_cache = dynamic_root, dynamic
        else:
            missing.append(key)  # reported; apt_imports.rs reports the import as MissingMovie
            continue
        movie, used = export_movie(source_cache, source / 'raw', key)
        sources[key] = source
        families |= used
        target = runtime / 'movies' / (key + '.json')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(movie, separators=(',', ':')) + '\n', encoding='utf-8')
        movies[key] = 'movies/' + key + '.json'
        queue.extend(i['file'] for i in movie['imports'])
    # Retail's secondary font (825D6B68: "Futura Shadow" -> native "futuraheavy", looked up in the whole
    # native font table, 82809208) is needed even when no movie in the set places that font itself.
    for family in sorted(families):
        partner = next((v for k, v in FONT_PAIRS.items() if k.lower() == family.lower()), None)
        if partner:
            families |= {name for name, row in mappings.items() if row['file_name'].lower() == partner.lower()}
    fonts = {family: cache.font_asset(family) for family in sorted(families)}
    # An APT font name without a FontManager row (trickguide's "HelveticaPresto") gets retail's fallback:
    # sub_82809208 matches AptName case-insensitively, else takes the first row whose FileName is not "debug"
    # (string 0x821A06E0). Row order = the collection's order; NOT RETAIL YET: the runtime table order is inferred.
    fallback = next((name for name, row in mappings.items() if row['file_name'].lower() != 'debug'), None)
    for family in sorted(families):
        if family not in mappings and fallback:
            mappings[family] = dict(mappings[fallback], fallback_of=fallback)
            fonts[family] = cache.font_asset(fallback)
    language = {row['label'].strip(): row['value'] for row in json.loads(
        (cache_root / 'metadata/languages/english_global.json').read_text(encoding='utf-8'))['entries']}
    # DLC text (the creator pack's replay editor labels, the map packs' location and world names) adds to the disc's;
    # a disc label keeps its text. Text comes from every pack with language tables, movies only from `extras`.
    for archive in dlc_text_archives(dlc, output / 'dlc') if dlc else []:
        for label, value in packages.language(archive).items():
            language.setdefault(label, value)
    # Script-made text fields name fonts at run time (sk8popup's createTextField body: "Futura Std Heavy"); the
    # runtime resolves names without a row to the same fallback, so it is exported with its font.
    if fallback and fallback not in fonts:
        fonts[fallback] = cache.font_asset(fallback)
    shared = {'language': language, 'fonts': fonts, 'font_mappings': mappings, 'font_pairs': FONT_PAIRS,
              'fallback_font': fallback}
    (runtime / 'shared.json').write_text(json.dumps(shared, separators=(',', ':')) + '\n', encoding='utf-8')
    # RGBA payloads referenced by shapes and fonts, copied from the cache.
    payloads = {}
    for key, file in movies.items():
        for shape in json.loads((runtime / file).read_text(encoding='utf-8'))['shapes'].values():
            for p in shape:
                if p.get('texture'):
                    payloads.setdefault(p['texture']['rgba'], sources[key])
    for f in fonts.values():
        if f:
            payloads.setdefault(f['texture'], cache_root)
    for relative, root in sorted(payloads.items()):
        source = _contained(root, relative)
        target = _contained(runtime, relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    # Named runtime images: a second, narrow extraction with fedynamic.big (its own cache, so every movie keeps its
    # source archive); without fedynamic.big the menus still export and the map pictures draw nothing.
    missing_images = None
    dlc_maps, picture_archives = {}, []
    if dlc:
        try:
            dlc_maps, picture_archives = dlc_world_maps(game, dlc, output / 'dlc-maps')
        except (FileNotFoundError, KeyError, ValueError) as error:
            missing_images = f'DLC map pictures: {error}'
    try:
        dynamic_root = output / 'cache-dynamic'
        extract_project(game, dynamic_root, include_dynamic=True, prefixes=IMAGE_PREFIXES_DYNAMIC + TEXTURE_PACKS_DYNAMIC,
                        update=True, extra_archives=tuple(picture_archives))
        images = named_images(dynamic_root, runtime, IMAGE_PREFIXES_DYNAMIC)
        images.update(pack_textures(dynamic_root, runtime, TEXTURE_PACKS_DYNAMIC))
    except FileNotFoundError as error:
        images, missing_images = {}, str(error)
    images.update(named_images(cache_root, runtime, IMAGE_NAMES_CORE))
    try:
        maps = world_maps(game, collections, output / 'db')
        # The packs' worlds after the disc's (a DLC row never replaces a disc row, as in asset_pipeline.dlc.merged).
        for key, name in dlc_maps.items():
            maps.setdefault(key, name)
    except (FileNotFoundError, KeyError, ValueError) as error:
        maps, missing_images = {}, (missing_images or '') + f' world maps: {error}'
    finally:
        shutil.rmtree(output / 'db', ignore_errors=True)
    # The credits' lines file (optional: without it the credits screen shows no lines).
    try:
        from vendor.skate3_ui.big import BigArchive
        from vendor.skate3_ui.project import find_big_directory
        lines = BigArchive(find_big_directory(game) / CREDITS_ARCHIVE)
        entry = next(e for e in lines.entries if e.path.lower() == CREDITS_ENTRY)
        (runtime / 'credits.txt').write_bytes(lines.read(entry))
    except (FileNotFoundError, StopIteration) as error:
        missing_images = (missing_images or '') + f' credits lines: {error!r}'
    files = {p.relative_to(runtime).as_posix(): _digest(p) for p in sorted(runtime.rglob('*')) if p.is_file()}
    manifest = {'format': FORMAT, 'version': VERSION, 'roots': list(roots), 'movies': movies,
                'missing_movies': missing, 'shared': 'shared.json',
                'unresolved_fonts': [f for f, a in fonts.items() if a is None],
                'images': dict(sorted(images.items())), 'world_maps': dict(sorted(maps.items())),
                'files': files}
    if missing_images:
        manifest['missing_images'] = missing_images
    (runtime / 'manifest.json').write_text(json.dumps(manifest, indent=1) + '\n', encoding='utf-8')
    print(f'Prepared {len(movies)} original menu movies ({len(payloads)} payloads): {runtime}', flush=True)
    return manifest


def runtime_files(root):
    """Verified {relative: sha256} of a prepared set (manifest last)."""
    manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
    if manifest.get('format') != FORMAT or manifest.get('version') != VERSION or not manifest.get('movies'):
        raise ValueError(f'Invalid menu movie set: {root}')
    files = {}
    for relative, sha256 in manifest['files'].items():
        path = _contained(root, relative)
        if _digest(path) != sha256:
            raise ValueError(f'Menu movie file hash mismatch: {path}')
        files[relative] = sha256
    files['manifest.json'] = _digest(root / 'manifest.json')
    return files


def install(assets, runtime):
    """Replaces assets/private/menu-movies with the verified set."""
    files = runtime_files(runtime)
    destination = assets / 'private' / 'menu-movies'
    staging = destination.with_name('menu-movies.new')
    if staging.exists():
        shutil.rmtree(staging)
    for relative in files:
        target = _contained(staging, relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(_contained(runtime, relative), target)
    runtime_files(staging)
    if destination.exists():
        shutil.rmtree(destination)
    staging.rename(destination)
    return {f'private/menu-movies/{relative}': sha for relative, sha in files.items()}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True, help='work folder (cache + runtime)')
    parser.add_argument('--collections', type=Path, required=True,
                        help='Owned private/stock/skater-collections.json')
    args = parser.parse_args()
    prepare(args.game.resolve(), args.output.resolve(), args.collections.resolve())
