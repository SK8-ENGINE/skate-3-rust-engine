---@meta
-- SDK 2 language-server declarations. Not executed at runtime.
---@alias Vec3 number[]
---@alias Quat number[] xyzw
---@class DeformationOptions
---@field yield_speed? number contact impulse/body mass threshold, m/s (default 2)
---@field compliance? number metres per m/s above yield (default 0.045)
---@field radius? number impact region metres (default 1.2)
---@field max_displacement? number cumulative offset limit metres (default 0.55)
---@field max_step? number per-impact crush metres (default 0.22)
---@field cooldown? number minimum update interval seconds (default 0.10)
---@field resolution? integer[] XYZ lattice counts, default {9,5,17}; <=2048 total
---@class BodyDesc
---@field deformation? DeformationOptions
---@field shape {type:'box'|'sphere'|'capsule'|'convex'|'mesh', half_extents?:Vec3, radius?:number, half_height?:number, points?:Vec3[], path?:string, object?:string}
---@field body_type 'dynamic'|'kinematic'|'static'
---@field mass? number
---@field position? Vec3
---@field heading? number
---@field friction? number
---@field ccd? boolean
---@field sensor? boolean
---@field membership? integer
---@field filter? integer
---@field center_of_mass? Vec3 body-local COM (weight transfer under corner loads)
---@field collider_offset? Vec3 chassis-local collider translation (legacy vehicle.json)
---@field inertia_half_extents? Vec3 optional inertia box; with COM sets MassProperties
---@field linear_damping? number default 0.08
---@field angular_damping? number default 0.5
---@class SpringRayOpts
---@field local_origin Vec3
---@field local_direction? Vec3 default {0,-1,0}
---@field rest_length number
---@field max_travel? number
---@field contact_radius? number
---@field stiffness number
---@field compression? number
---@field relaxation? number
---@field damping? number alias for compression/relaxation
---@field max_force? number
---@field dt number
---@class SpringRayHit
---@field in_contact boolean
---@field point Vec3
---@field normal Vec3
---@field hard_point Vec3
---@field direction_ws Vec3
---@field suspension_length number
---@field load number
---@field relative_velocity number
---@class BodySnapshot
---@field player_overlapping boolean Enabled native local board/skater volumes touching this body; includes sensors. Solid contact margin 0.025m; sensors require intersection. Updated each fixed callback, false while suspended/attached. Geometric query only; collision masks do not suppress it.
---@field position Vec3
---@field rotation Quat
---@field linvel Vec3
---@field angvel Vec3
---@field force Vec3 last sdk.physics.force this tick (zeros if none); Rapier user forces are cleared each tick
---@field torque Vec3 last sdk.physics.torque this tick
---@field mass number
---@field speed number |linvel|
---@class PlayerSnapshot
---@field landed_trick_base string Engine-authored base label identifier (not localized), captured with landing_seq
---@field landed_spin_degrees integer Signed body rotation from the settled scorer; excludes board shuvit rotation
---@field landed_clean boolean Quality captured at the confirmed landing
---@field landed_sketchy boolean Sketchy quality captured at the confirmed landing
---@field suspended boolean local or remote simulation/visibility suspension
---@field id? string
---@field local? boolean
---@field name? string display name from the multiplayer menu
---@field position Vec3
---@field velocity Vec3
---@field angvel Vec3
---@field forward Vec3
---@field heading number
---@field rotation Quat
---@field speed number
---@field on_board boolean
---@field state integer
---@field category integer
---@field filtered integer
---@field mode string ground|air|grind|offboard|offboard_air|bail|teleport
---@field grind string|nil
---@field bailing boolean
---@field trick string HUD trick name; empty when idle
---@field trick_seq integer increments each newly announced trick; NOT a landing
---@field landing_seq integer monotonic confirmed, banked on-board sequence counter
---@field landed_trick string last confirmed landing label; persists until another landing
---@field bail_seq integer monotonic wipeout-entry counter
---@field new_trick boolean true for the announce frame
---@field modified_trick boolean
---@field close_tricks boolean
---@field sequence boolean combo/line still live
---@field score number
---@field line number
---@field multiplier number
---@field line_time number
---@field clean boolean
---@field sketchy boolean
---@field switch boolean
---@field fakie boolean
---@field nollie boolean
---@field intents table<string, number>
---@class ContactEvent
---@field a string|nil body key or `"ground"` when the other side is owned
---@field b string|nil body key or `"ground"`
---@field started boolean true on pair begin, false on end
---@class TouchingPair
---@field a string|nil body key or `"ground"`
---@field b string|nil body key or `"ground"`
---@class PadSnapshot
---@field buttons integer XInput button bits
---@field triggers number[] LT, RT in 0..1
---@field left number[] stick XY
---@field right number[] stick XY
---Identity of one controller slot (read-only; capability `controllers` = 1).
---@class ControllerKind
---@field family 'xbox360'|'xbox_one'|'xbox_elite'|'xinput_gamepad'|'playstation3'|'playstation4'|'playstation5'|'switch_pro'|'joycon_left'|'joycon_right'|'joycon_pair'|'standard'|'wheel'|'arcade_stick'|'flight_stick'|'dance_pad'|'guitar'|'drum_kit'|'arcade_pad'|'unknown'
---@field name string device name (SDL) or model name
---@field vendor_id? integer USB vendor id (e.g. 0x045e Microsoft)
---@field product_id? integer USB product id (e.g. 0x0b22 Elite Series 2 over Bluetooth LE)
---@field backend 'sdl'|'xinput'
---@field driver string SDL device path ("XInput#0" = SDL's XInput driver) or "XInput"
---@field xinput_subtype? integer XINPUT_DEVSUBTYPE_* (XInput backend only)
---@field wireless? boolean XInput backend only
---@field paddles integer back paddles the driver reports (usable in settings/controller.json)
---@field hardware_paddles integer back paddles the model has (0 = none / unknown)
---@field touchpad boolean
---@field misc_button boolean Share / Capture / Mute button reported
---@field prompt_style 'xbox'|'playstation'|'nintendo' printed face-button names (layout stays positional)
---@field face_labels string[] printed names of the South, East, West, North buttons
---@field summary string one-line description (as in the log and the Esc menu)
---@class NetworkInfo
---@field active boolean
---@field local_id string
---@field is_host boolean
---@field host_id string
---@field players string[]
---@field states table<string, table<string, table<string, any>>> mod_id → peer_id → key → value
---@field status string
---@class FrameSnapshot read-only frame-time statistics of the presented frames (engine `frame_timing`)
---@field frame integer presented frames since start
---@field ms number last frame time (real time between frame starts)
---@field fixed_steps integer physics (fixed 60 Hz) steps run in the last frame; >1 = catching up after a slow frame
---@field main_ms number CPU time of the engine's main-thread schedules in the last frame (much less than `ms` = waiting on rendering/GPU)
---@field fixed_ms number CPU time of the physics steps in the last frame
---@field hitch boolean last frame took more than 2x the median of the previous 120 frames
---@field window_s number seconds covered by the statistics below (5)
---@field frames integer frames in that window
---@field fps number mean frames per second over the window
---@field mean_ms number
---@field median_ms number
---@field low_1_ms number 1 % low: the slowest 1 % of frames take at least this long (99th percentile)
---@field low_01_ms number 0.1 % low (99.9th percentile)
---@field worst_ms number longest frame in the window
---@field hitches integer hitch frames in the window
---@class SDKSnapshot
---@field player PlayerSnapshot
---@field skaters table<string, PlayerSnapshot>
---@field map {name:string, generation:integer}
---@field tick integer
---@field keys table<string,boolean>
---@field actions number[]
---@field pad PadSnapshot
---@field controllers {active?:integer, slots:(ControllerKind|nil)[]} slot 0..3 at index 1..4
---@field paused boolean
---@field replay boolean
---@field frame FrameSnapshot frame-time statistics, read-only; refreshed 4x per second (ms/fixed_steps/hitch every frame)
---@field camera? {position:Vec3}
---@field camera_angle? CameraAngleState
---@field attach? {body:string, owner:string}
---@field physics {bodies:table<string,BodySnapshot>, contacts:ContactEvent[], touching:TouchingPair[]}
---@field network? NetworkInfo
---@class ModCallbacks
---@field on_load? fun()
---@field on_unload? fun()
---@field on_ui_update? fun(event:{dt:number,paused:boolean}) runs while paused; does not advance simulation timers (menus >= 2)
---@field on_update? fun(event:{dt:number})
---@field on_fixed_update? fun(event:{dt:number})
---@field on_settings? fun(event:{key:string,value:any})
---@field on_event? fun(event:{name:string})
sdk = {
    api_version = 2,
    ---@type string
    mod_id = "",
    ---@type table<string, any>
    settings = {},
    ---@type SDKSnapshot
    snapshot = {},
    physics = {}, graphics = {}, player = {}, camera = {}, input = {}, ui = {}, net = {}, assets = {},
    time = { elapsed = 0 },
}
---@param text string
function sdk.log(text) end
---@param path string
---@return string
function sdk.read_text(path) end
---@param path string package-relative .glb
---@return {nodes:string[], meshes:string[]}
function sdk.assets.objects(path) end
---@param key string
---@param body BodyDesc
function sdk.physics.spawn(key, body) end
---@param key string
function sdk.physics.remove(key) end
---@param key string
---@param opts {shape:{type:'mesh'|'convex', path?:string, object?:string, points?:Vec3[]}, position?:Vec3, friction?:number}
function sdk.physics.add_collider(key, opts) end
---@param key string
---@param force Vec3
---@param point? Vec3
function sdk.physics.force(key, force, point) end
---@param key string
---@param impulse Vec3
---@param point? Vec3
function sdk.physics.impulse(key, impulse, point) end
---@param key string
---@param torque Vec3
function sdk.physics.torque(key, torque) end
---@param key string
---@param torque Vec3
function sdk.physics.torque_impulse(key, torque) end
---@param origin Vec3
---@param direction Vec3
---@param opts? {max_distance?:number, filter?:'all'|'ground', exclude?:string[]}
---@return {body:string|nil, point:Vec3, normal:Vec3, toi:number}|nil
function sdk.physics.raycast(origin, direction, opts) end
---@param key string
---@param point Vec3
---@return Vec3|nil
function sdk.physics.velocity_at(key, point) end
---@param key string
---@param point Vec3
---@param direction Vec3
---@return number|nil
function sdk.physics.effective_inv_mass(key, point, direction) end
---@param key string
---@param opts SpringRayOpts
---@return SpringRayHit|nil
function sdk.physics.spring_ray(key, opts) end
---@param key string
---@param local_accel Vec3
---@param dt? number
---@return Vec3|nil
function sdk.physics.local_ang_accel_impulse(key, local_accel, dt) end
---@param key string
---@param linvel Vec3
function sdk.physics.set_linvel(key, linvel) end
---@param key string
---@param angvel Vec3
function sdk.physics.set_angvel(key, angvel) end
---@param key string
---@param position Vec3
---@param rotation Quat
function sdk.physics.set_pose(key, position, rotation) end
---@param key string
---@param joint {body_a:string, body_b:string, anchor_a:Vec3, anchor_b:Vec3, axis?:Vec3, limits?:[number,number], contacts_enabled?:boolean}
function sdk.physics.revolute(key, joint) end
---@param key string
---@param joint {body_a:string, body_b:string, anchor_a?:Vec3, anchor_b?:Vec3, axis?:Vec3, limits?:[number,number], contacts_enabled?:boolean}
function sdk.physics.prismatic(key, joint) end
---@param key string
---@param motor {mode:'velocity', target_velocity:number, factor?:number, max_force?:number}|{mode:'position', target_position:number, stiffness:number, damping:number, max_force?:number}
function sdk.physics.joint_motor(key, motor) end
---@param key string
---@param spring {position?:number, target_position?:number, stiffness:number, damping:number, max_force?:number}
function sdk.physics.joint_spring(key, spring) end
---@param key string
function sdk.physics.remove_joint(key) end
---@param key string
---@return BodySnapshot|nil
function sdk.physics.read(key) end
---@return ContactEvent[] edge events from the previous Rapier step
function sdk.physics.contacts() end
---@return TouchingPair[] pairs in contact after the previous Rapier step (`ground` included)
function sdk.physics.touching() end
---@param key string
---@param opts {path?:string, body?:string, position?:Vec3, rotation?:Quat, scale?:Vec3, color?:Vec3, visible?:boolean, opacity?:number}
-- opts.deform_nodes: optional string[] of GLB scene node names whose meshes follow the bound body deformation.
function sdk.graphics.mesh(key, opts) end
---@param key string
function sdk.graphics.remove(key) end
---@param key string
---@param visible boolean
function sdk.graphics.set_visible(key, visible) end
---@class MeshBufferOpts
---@field body? string
---@field position? Vec3 body-local when bound, world when unbound
---@field rotation? Quat
---@field scale? Vec3
---@field blend? boolean default true
---@field unlit? boolean default true
---@field visible? boolean default true
---@field depth_bias? number decal bias, default 0
---@field texture? string mod-relative PNG path, e.g. textures/skid_tread.png
---@field capture? string named camera capture; mutually exclusive with texture
---@field tint? Vec3 material tint, default white
---@class MeshBufferWrite
---@field positions Vec3[]
---@field normals? Vec3[]
---@field colors? number[][] per-vertex RGBA (packed flat by the runtime)
---@field uvs? number[][]
---@field indices integer[] 0-based triangle indices
---@class LightOpts
---@field kind? "point"|"spot"
---@field body? string
---@field position? Vec3
---@field offset? Vec3
---@field direction? Vec3 spot axis
---@field color? Vec3
---@field intensity? number
---@field range? number
---@field inner_angle? number
---@field outer_angle? number
---@param key string
---@param opts? MeshBufferOpts
function sdk.graphics.mesh_buffer(key, opts) end
---@param key string
---@param data MeshBufferWrite
function sdk.graphics.mesh_buffer_write(key, data) end
---@param key string
---@param data MeshBufferWrite append-only delta; indices are absolute in the combined mesh
function sdk.graphics.mesh_buffer_append(key, data) end
---@param key string
---@param opts? LightOpts
function sdk.graphics.light(key, opts) end
---@return PlayerSnapshot
function sdk.player.read() end
---Freeze and hide the local skater and remove their collision participation.
---False releases only this mod's suspension; unload/world change also releases it.
---@param suspended boolean
function sdk.player.suspend(suspended) end
---@return table<string, PlayerSnapshot>
function sdk.player.skaters() end
---@param id string|number
---@return PlayerSnapshot|nil
function sdk.player.skater(id) end
---@param body string
---@param offset? Vec3
function sdk.player.attach(body, offset) end
function sdk.player.detach() end
---@return string|nil
function sdk.player.attached() end
---@param body? string
---@param offset? Vec3
function sdk.camera.follow(body, offset) end
function sdk.camera.clear_follow() end
---@param position Vec3
---@param look_at? Vec3
function sdk.camera.set(position, look_at) end
---@param peer string|number|nil peer whose actual camera transform/FOV to mirror; nil restores native camera
function sdk.camera.watch(peer) end
---@class CameraAngleState
---@field selected '"low"'|'"high"' the player's Camera Angle setting
---@field active '"low"'|'"high"' the angle the stock camera graph uses now (a mod may force it)
---@field owner string|nil mod forcing the angle
---@field shot string current stock camera shot (e.g. "bl_chase" low, "bl_high_chase" high)
---@field tuned table<string,string> tuned shot -> owning mod
---@return CameraAngleState
function sdk.camera.angle() end
---Force the retail Camera Angle; nil hands it back to the player's setting (capability camera >= 4).
---@param angle '"low"'|'"high"'|nil
function sdk.camera.set_angle(angle) end
---@class CameraShotTuning retail camera_shots attributes, in their units (metres, degrees, seconds)
---@field PositionDistance? number
---@field PositionElevation? number
---@field PositionHeading? number
---@field FramingLensLength? number
---@field FramingRoll? number
---@field FramingYaw? number
---@field FramingPitch? number
---@field ReferenceBoardOffset? number
---@field SmoothingDirection? number
---@field SmoothingElevation? number
---@field SmoothingYaw? number
---@field SmoothingPitch? number
---@field TransitionTime? number
---Replace stock values of one camera shot; nil restores it. Unknown shots fail the command.
---@param shot string stock shot name, lower case
---@param patch CameraShotTuning|nil
function sdk.camera.tune_shot(shot, patch) end
---@param key string
---@return boolean
function sdk.input.down(key) end
---Current value of a mapped gameplay action (ID 64..81 or a key of `sdk.input.action_ids`).
---@param id integer|string
---@return number
function sdk.input.action(id) end
---@return PadSnapshot
function sdk.input.pad() end
---@param key string
---@param text string
function sdk.ui.text(key, text) end
---Local-only text in Pause > Multiplayer > Debug; never draws a gameplay overlay.
---Up to 8 entries per mod, 1024 UTF-8 bytes each. Empty text removes an entry.
---@param key string
---@param text string
function sdk.ui.multiplayer_debug(key, text) end
---@return NetworkInfo
function sdk.net.info() end
---@return string[]
function sdk.net.players() end
---@param key string
---@param value any JSON-compatible; at most 512 encoded bytes; nil clears
function sdk.net.publish(key, value) end
---@param peer string|number peer id from info().local_id / remote peers
---@param key string
---@return any|nil
function sdk.net.read(peer, key) end


---@class TeleportOptions
---@field position Vec3 world position, each coordinate within +/-100000
---@field heading? number radians, default 0
---@field velocity? Vec3 world linear velocity, each component within +/-200
---@param options TeleportOptions moves only the local player through native travel
function sdk.player.teleport(options) end

sdk.session = {}
---@return {active:boolean, local_id:string, is_host:boolean, authority:string, players:string[]}
function sdk.session.info() end
---The transport host may reclaim authority; the current authority may renew it.
function sdk.session.claim() end
---@param peer string|number connected peer; transfer is ratified asynchronously by host
function sdk.session.transfer(peer) end
---@param peer string|number connected peer; requires session authority
---@param options TeleportOptions
function sdk.session.teleport(peer, options) end

sdk.volumes = {}
---@param key string mod-owned id; at most 32 boxes per mod
---@param options {position:Vec3, size:Vec3, rotation?:Quat, visible?:boolean, color?:Vec3, opacity?:number}
function sdk.volumes.box(key, options) end
---@param key string
function sdk.volumes.remove(key) end
---@param key string
---@return {position:Vec3,size:Vec3,rotation:Quat,inside:string[]}|nil point overlaps, not physical collisions
function sdk.volumes.read(key) end

---@class TriggerVolume
---@field id string retail instance id (16 hex digits), custom-map id or "mod:<mod>:<key>"
---@field name string short name, e.g. "tut_sksc_reset_vol01"
---@field full_name string|nil retail editor path
---@field group "challenge"|"stairs"|"camera"
---@field source "map"|"mod"
---@field owner string|nil mod id for mod volumes
---@field instance_id string|nil
---@field link_guid string|nil
---@field center Vec3
---@field axes Vec3[] box axes in world space
---@field rotation Quat
---@field half_extents Vec3
---@field fatness number
---@field aabb_min Vec3
---@field aabb_max Vec3
---@field enabled boolean false while a mod switched it off
---@field inside string[] tracked bodies inside ("player", "mod:<mod>:<key>")

sdk.triggers = {}
---@return TriggerVolume[]
function sdk.triggers.list() end
---@param id string id or short name
---@return TriggerVolume|nil
function sdk.triggers.get(id) end
---@param body? string default "player"
---@return string[] volume ids
function sdk.triggers.inside(body) end
---Events: on_event {name="trigger_entered"|"trigger_exited", body, volume, volume_name, group, instance_id, link_guid}.
---@param key string mod-owned id; at most 64 per mod
---@param options {center:Vec3, half_extents:Vec3, rotation?:Quat, name?:string, group?:"challenge"|"stairs"|"camera"}
function sdk.triggers.box(key, options) end
---@param key string
function sdk.triggers.remove(key) end
---@param id string map volume id; the switch ends when this mod stops
---@param enabled boolean
function sdk.triggers.set_enabled(id, enabled) end
---@param key string one of this mod's physics bodies; at most 16
---@param options? {radius?:number, length?:number}
function sdk.triggers.track(key, options) end
---@param key string
function sdk.triggers.untrack(key) end
---@param options? {radius?:number, length_scale?:number, length_pad?:number, foot_pad?:number} nil restores retail (0.34, 0.5, 0.05, 0.02)
function sdk.triggers.configure(options) end

---@param key string mod-owned id; at most 2 per mod, 4 total
---@param options {position:Vec3,look_at:Vec3,fov?:number,width?:integer,height?:integer} fov radians [0.2,2.5]; sizes [64,512], multiples of 16
function sdk.camera.capture(key, options) end
---@param key string releases the named target; bound meshes lose their image
function sdk.camera.clear_capture(key) end


---@class MenuItem
---@field id string unique within this menu
---@field label string
---@field description? string
---@field enabled? boolean default true
---@field children? MenuItem[] submenu, up to 4 levels
---@param key string mod-owned id; up to 8 menus per mod
---@param options {title:string,section?:string,items:MenuItem[]} up to 64 total items
---Actions dispatch on_event {name="menu_action",menu=key,item=item.id} to the owner, including while paused.
function sdk.ui.menu(key, options) end
---@param key string
function sdk.ui.remove_menu(key) end

-- Retail menus (capability `retail_menus` = 1; docs/hails-additions/31-retail-menus.md).
-- The pause menu (crossbar) and Game Settings come from the retail tables extracted at setup.
-- Entries are keyed by retail internal names: categories (tabs) `SinglePlayer`, `Multiplayer`,
-- `Create`, `Learn`, `Options`; items `ChallengeMap`, `GameSettings`, `SkateFeed`, ...; settings
-- rows `ID_GAMESETTINGS_SFXVOLUME`, .... Modes: `Career`, `CareerPark`, `FreePlay`,
-- `FreePlayPark`, `FreeSkateRestricted` (and the lobby / challenge modes). Every retail entry
-- exists; one without a handler is greyed. New entries a mod adds are named `<mod id>.<name>`.
-- Handled entries send on_event {name="menu_select"|"menu_highlight"|"menu_value",
-- entry=kind, id=id, mode=mode key, value=new value (menu_value)}. All edits, handlers and
-- values of a mod are removed when it stops.
sdk.menus = {}

---@class MenuEntryOptions
---@field label? string language string id or plain text
---@field icon? string a retail icon label (map, settings, skatefeed, replay, profile, extras, ...)
---@field image? string an image inside the mod (wins over icon; drawn once the menus render)
---@field help? string helper text
---@field category? string item: the tab it goes into (required for a new item)
---@field modes? string[] mode keys; empty = every mode
---@field position? integer 0-based position in the tab / screen / tab bar
---@field widget? 'option'|'slider'|'selector' setting: required for a new row
---@field screen? integer setting: settings screen index (0..12 retail; required for a new row)

---@alias MenuRule boolean|{fact?:string, eq?:number, ne?:number, lt?:number, le?:number, gt?:number, ge?:number, modes?:string[], all?:MenuRule[], any?:MenuRule[], not?:MenuRule}

---@class MenuHandleOptions
---@field select? boolean receive menu_select (default true); this enables the entry
---@field highlight? boolean receive menu_highlight
---@field value? boolean|number a settings row's value, kept by the game (menu_value on change)
---@field enabled? MenuRule when the entry is enabled (over retail's rule)
---@field visible? MenuRule when the entry is shown (over retail's rule)
---@field confirm? {title:string, description?:string, yes?:string, no?:string, when?:MenuRule} ask before select
---@field rule? {kind:"slider"|"toggle"|"cycle", min?:number, max?:number, step?:number, snap?:number, bars?:number, count?:integer, labels?:string[]} Left / Right rule for a settings row (retail rules are the default; reverted on disable)

---@param id string
---@param options? MenuEntryOptions
function sdk.menus.category(id, options) end
---@param id string
---@param options? MenuEntryOptions
function sdk.menus.item(id, options) end
---@param id string
---@param options? MenuEntryOptions
function sdk.menus.setting(id, options) end
---@param entry 'category'|'item'|'setting'
---@param id string
---@param modes? string[] empty = everywhere
function sdk.menus.hide(entry, id, modes) end
---@param entry 'category'|'item'|'setting'
---@param id string
---@param options? MenuHandleOptions
function sdk.menus.handle(entry, id, options) end
---@param entry 'category'|'item'|'setting'
---@param id string
function sdk.menus.unhandle(entry, id) end
--- Set a settings row's value. On the mod's own bound row it acts like a menu change (menu_value
--- event). On any other row (an engine setting such as a volume, Camera Angle or Play Mode, or
--- another mod's row) the value goes into this mod's own layer on top: shown and applied while the
--- mod runs, never saved to the player's settings, and removed when the mod stops, so the previous
--- value comes back. The last mod to write wins.
---@param id string a settings row
---@param value boolean|number
function sdk.menus.set_value(id, value) end
---@param id string a settings row
---@return boolean|number|nil
function sdk.menus.value(id) end
---@return {mode:string, loaded:boolean, values:table<string, boolean|number>}
function sdk.menus.info() end

---Supply or replace a front-end APT movie by its retail import name (e.g. 'source/controls/panel')
---with a JSON file in the mod's folder, in the format setup exports to assets/private/menu-movies
---(movies/<name>.json). Menus importing it are relinked; the newest mod wins; removed when the mod
---stops. Loaded state: sdk.menus.info().movies (loaded, imports, failures, mods).
---@param name string
---@param path string
function sdk.menus.movie(name, path) end

---Replace one bitmap of a front-end APT movie by its stable id: the movie's import name (e.g.
---'source/screens/main/core_menu') plus the bitmap character id (listed in
---sdk.menus.info().movies.bitmaps), with a PNG in the mod's folder (up to 2048 x 2048). The image
---covers the same fill area as the retail bitmap (same UVs, sampled clamped or wrapped as the retail
---unit is). The newest mod wins; removed when the mod stops.
---@param movie string
---@param bitmap integer
---@param path string
function sdk.menus.bitmap(movie, bitmap, path) end

---@class NativeContact
---@field a {kind:string,index?:integer} board, skater, external, or world
---@field b {kind:string,index?:integer}
---@field point Vec3 world contact point
---@field normal Vec3 native A-side normal
---@field force Vec3 native solved normal plus friction force
---@field normal_force Vec3
---@field friction_force Vec3
---@field impulse Vec3 force times simulation dt
---@field static_friction number combined contact coefficient
---@field dynamic_friction number combined contact coefficient
---@field material_tags integer[] A and B material tags
---@field id string stable body pair, shared by manifold points
---@field phase 'begin'|'stay'|'end'
---@field relative_velocity_before_solve Vec3 A minus B at contact point, including angular velocity
---@field closing_speed number m/s; zero on end
---@class NativeJoint
---@field index integer zero-based native joint ID (use this, not Lua array index)
---@field name string native authored joint name
---@field parent integer native part index
---@field child integer native part index
---@field swing_limit number radians
---@field twist_limit number radians
---@field free_swing boolean
---@field free_twist boolean
---@field drive_enabled boolean false when a mod suppresses the child's animation drives
---@field override_owner? string
---@field enabled boolean
---@field possession_enabled boolean
---@field load? NativeJointLoad most recent eligible solved row
---@field parameters integer[] native words, read-only
---@field frames integer[] native words, read-only
---@return {tick:integer,dt:number,contacts:NativeContact[],joints:NativeJoint[],parts:table[],ragdoll:boolean,partial_ragdoll:boolean} local player only
function sdk.player.physics() end
---@return NativeContact[] most recent solved frame, max 128 active plus end reports to reach 256
function sdk.player.contacts() end
---@return NativeJoint[]
function sdk.player.joints() end
---@return {index:integer,position:Vec3,velocity:Vec3,angvel:Vec3,inverse_mass:number}[]
function sdk.player.parts() end
---@param joint integer zero-based index from sdk.player.joints()
---@param options JointOverride angles [0.01,pi], radians
---Replaces this mod's override; omitted properties follow native state. Another mod cannot take an owned joint.
function sdk.player.set_joint(joint, options) end
---@param joint integer restores this mod's joint override
function sdk.player.reset_joint(joint) end
---Restores all joint AND part overrides owned by this mod.
function sdk.player.reset_joints() end

-- General engine access: see GENERAL_API.md for units, lifecycle and limitations.
sdk.commands = {}
---@class CommandResult
---@field token integer latest request token for this key
---@field ok boolean host execution succeeded; not remote acknowledgement
---@field error? string
---@field value? any catalog for engine_inspect, otherwise nil
---@field tick integer
---@param key string at most 64 result keys per mod
---@param command table validated native command; no nested request
---@return integer token
function sdk.commands.request(key, command) end
---@param key string
---@return CommandResult|nil nil until the latest token is observed
function sdk.commands.result(key) end

sdk.engine = {version=1}
---@return string[]
function sdk.engine.systems() end
---@param system string player, rig, bodies, input, graphs, animation, scoring, world, camera, network, commands
---@return table|nil latest snapshot
function sdk.engine.read(system) end
---@param key string command result key
---@param system 'graphs'|'scoring'|'audio_catalog'|string `audio_tuning:<domain>[/path]` reads a tuning domain
function sdk.engine.inspect(key, system) end

---@class BodyReference
---@field kind 'skater'|'board'|'mod'
---@field index? integer zero-based native physical body ID
---@field key? string mod-owned Rapier body key
---@class NativeBody: BodyReference
---@field position Vec3 world centre of mass
---@field rotation Quat XYZW
---@field velocity Vec3 world m/s
---@field angvel Vec3 world rad/s
---@field inverse_mass number
---@field inverse_inertia Vec3[] world matrix columns
---@field state_flags integer effective native solve state
---@field name? string mapped animation bone name
---@field joint? integer native joint whose child is this body
---@field collision_enabled? boolean effective enabled flag
---@field animation_drives? boolean permits native animation drives
---@field possession_drives? boolean permits native board-holding drives
---@field material? {static_friction:number,dynamic_friction:number}
---@field override? {owner:string,options:PartOverride}
sdk.bodies = {}
---@param ref BodyReference
---@return NativeBody|table|nil
function sdk.bodies.read(ref) end
---@param ref BodyReference
---@param value Vec3 world N s
---@param point? Vec3 world point; omitted means centre of mass
function sdk.bodies.impulse(ref, value, point) end
---@param ref BodyReference
---@param value Vec3 world N m s
function sdk.bodies.angular_impulse(ref, value) end

---@class JointOverride
---@field swing_limit? number [0.01,pi] radians
---@field twist_limit? number [0.01,pi] radians
---@field free_swing? boolean
---@field free_twist? boolean
---@field enabled? boolean false removes entire constraint including linear attachment
---@field drive_enabled? boolean false suppresses child animation drives
---@field possession_enabled? boolean false suppresses child board-holding drives
---@field descendants? boolean extends drive suppressions to child subtree
---@class PartOverride
---@field motion? 'dynamic'|'frozen'|'static' solve flags 4/2/1; not a player state change
---@field collision? boolean
---@field friction? number [0,10], both material coefficients
---@field animation_drives? boolean
---@field possession_drives? boolean
---@class NativeJointLoad
---@field linear_impulse Vec3 world N s on child/A
---@field angular_impulse Vec3 world N m s, direct angular couple
---@field force Vec3 N
---@field torque Vec3 N m, excluding linear anchor lever-arm contribution
---@field solver_words integer[] packed native u32 values
---@class NativeRig
---@field tick integer completed solve cursor
---@field dt number seconds
---@field parts NativeBody[]
---@field board NativeBody[]
---@field joints NativeJoint[]
---@field contacts NativeContact[] at most 128 active, plus end reports to reach 256
---@field contacts_truncated boolean observation rows were omitted; pair tracking continues
---@field ragdoll boolean native mode
---@field partial_ragdoll boolean native mode
sdk.rig = {}
---@return NativeRig
---@param fields? string[] Optional top-level rig fields; omit for the complete rig.
function sdk.rig.read(fields) end
---@return NativeBody[]
function sdk.rig.parts() end
---@return NativeJoint[]
function sdk.rig.joints() end
---@return NativeContact[]
function sdk.rig.contacts() end
---@param index integer native joint ID, not a Lua array index
---@param options JointOverride replacement override; omitted fields follow native settings
function sdk.rig.configure_joint(index, options) end
---@param index integer native joint ID
function sdk.rig.reset_joint(index) end
---@param index integer physical skater part ID
---@param options PartOverride
function sdk.rig.configure_part(index, options) end
---@param index integer physical skater part ID
function sdk.rig.reset_part(index) end
---Restores all joint AND part overrides owned by this mod.
function sdk.rig.reset() end

---Overrides one mapped gameplay action (retail input.cfg GP_* actions; table in GENERAL_API.md):
---64 left_stick_x  GP_LStickX  steer, kick-turn, body spin, powerslide, grind balance
---65 left_stick_y  GP_LStickY  steering angle, automatic push, off-board walking
---66 left_stick_click  GP_LStickIn  bail (with 69 and both triggers full)
---67 right_stick_x  GP_RStickX  flick-it tricks, tweaks, grinds, off-board look
---68 right_stick_y  GP_RStickY  flick-it tricks, manuals, tweaks, off-board look
---69 right_stick_click  GP_RStickIn  bail (with 66), off-board air body tweak
---70 left_trigger  GP_LTrigger  left-hand grab, crouch; full press off board drops the board
---71 right_trigger  GP_RTrigger  right-hand grab, crouch; full press off board throws the board
---72 left_bumper  GP_LBumper  wins over RB; held, blocks D-pad gestures and the board toggle
---73 right_bumper  GP_RBumper  grab the world (ledges, handplants), dark catch
---74..77 dpad_up / dpad_down / dpad_left / dpad_right  gameplay gestures
---78 x  GP_XFace  push (left foot), off-board jump, recover after a bail
---79 y  GP_YFace  get off / back on the board
---80 a  GP_AFace  push (right foot), off-board sprint, recover after a bail
---81 b  GP_BFace  brake, dismount / no-foot air, dark catch
---@param id integer|string action ID 64..81 or a key of `sdk.input.action_ids`
---@param value? number [-1,1] (buttons: 1 pressed; triggers 0..1, full = 1); nil restores normal input
function sdk.input.override_action(id, value) end
---Mapped gameplay action IDs by key (capability `action_ids` = 1).
---@type table<string, integer>
sdk.input.action_ids = {
    left_stick_x = 64, left_stick_y = 65, left_stick_click = 66,
    right_stick_x = 67, right_stick_y = 68, right_stick_click = 69,
    left_trigger = 70, right_trigger = 71, left_bumper = 72, right_bumper = 73,
    dpad_up = 74, dpad_down = 75, dpad_left = 76, dpad_right = 77,
    x = 78, y = 79, a = 80, b = 81,
}
---Identity of the controller in a slot (capability `controllers` = 1). Read-only.
---@param slot? integer 0..3; default = the slot gameplay reads
---@return ControllerKind|nil nil when the slot is empty
function sdk.input.controller(slot) end
---All four slots and the slot gameplay reads.
---@return {active?:integer, slots:(ControllerKind|nil)[]}
function sdk.input.controllers() end
sdk.graphs = {}
---@param graph 'action'|'motion'
---@return {current?:integer,previous?:integer,name?:string,dt:number,state_times:table,active_behaviors:integer[]}|nil
function sdk.graphs.read(graph) end
---@param graph 'action'|'motion'
---@param target 'state'|'transition'|'behavior'
---@param index integer runtime ID from the catalog; zero-based
---@param enabled? boolean nil restores the original gate
function sdk.graphs.set_enabled(graph, target, index, enabled) end

---Feature discovery: extension name → version, compiled into the engine (an older engine lacks
---newer keys, so test `(sdk.capabilities.audio or 0) >= 1` before relying on a feature). The
---manifest API stays 2. Includes among others `audio`, `world_audio`, `engine_access`,
---`command_results`, `player_physics`, `menus`, `camera`, `volumes`, `capture`.
---@type table<string, integer>
sdk.capabilities = {}

-- Audio extension 1 (capability `audio`): the mod's own sounds. Keys and files belong to the calling
-- mod. Files: mod-relative PCM16 WAV (`.wav`, no `..`, `\`, `:`, `#`, `?`), 1–2 channels, 8–48 kHz,
-- at most 30 s and 8 MiB each; metadata chunks are dropped. Limits: 32 clips / 32 MiB / 32 voices per
-- mod, 128 / 128 MiB / 128 in all. By default (audio >= 3) a voice plays through the game's native
-- mixer (see `native`); a Bevy voice (`native = false`, or no free native voice) plays outside the
-- game's native audio engine: master volume and `--mute` apply, but no retail reverb, distance curves
-- or ducking. They pause while the
-- game is paused or a replay runs, and stop when the mod is disabled, reloaded or fails (clips are
-- released then too). A voice bound to a body stops when that body is removed.
---@class AudioPlayOptions
---@field path string mod-relative PCM16 WAV
---@field body? string follow one of this mod's physics bodies (exclusive with position)
---@field position? Vec3 world position of an unbound voice (default {0,0,0})
---@field offset? Vec3 added to the body pose or the position, -100..100 m per axis (default {0,0,0})
---@field loop? boolean default false
---@field volume? number 0..1 (default 1)
---@field pitch? number playback speed 0.25..4: changes pitch and duration (default 1)
---@field spatial? boolean positional (default true); false plays at the listener
---@field spatial_scale? number 0.001..1, world metres → spatial units (default 0.1)
---@field paused? boolean start paused (default false)
---@field fade_in? number seconds 0..2 (default 0.01)
---@field native? boolean audio >= 3: the game's native mixer. **Default (nil): native** while the game's
---native audio runs and one of its 24 native voices is free, else the Bevy voice above; `true` = native
---only (an error without it: use `sdk.commands.request` to get it as a result); `false` = always the
---Bevy voice. The WAV joins a per-mod bank; the voice follows the retail world-emitter law: dry level
---and pan from a MixMap Emitter instance, the environment (reverb) send rolling off with camera
---distance 4 → 70 m, and the sound's own reach (`falloff`, the `.ems` record test). Category volume:
---`group`. 24 native voices in all (they count in the 32 / 128 voice limits and share the mixer with
---the game). `spatial_scale` is for the Bevy voice only (ignored by a native one).
---@field falloff? AudioFalloff native, positional: the reach (default {radius = 40, curve = 'squared'},
---retail's traffic-car reach: the vehicle list is cut at 40 m). Refused with `native = false`.
---@field reverb? boolean native: send into the environment bus (default true, as retail emitters)
---@field group? 'world'|'player' native: Ambience volume (`world`, default) or Effects volume
---@class AudioFalloff
---@field radius number reach in metres (0.1..10000): silent at and beyond it
---@field core? number inner fraction 0..1 at full level (default 0)
---@field curve? 'squared'|'linear'|'flat' retail eVolumeFalloffType 0 / 1 / 2 (default 'squared')
---@class AudioUpdateOptions
---@field volume? number 0..1
---@field pitch? number 0.25..4
---@field paused? boolean
---@field position? Vec3 unbound voices only
---@field offset? Vec3
---@type {version:integer}
sdk.audio = { version = 3 }
---Load and validate a WAV now so the first play doesn't read it. A bad file, an unknown body or a
---full limit fails the mod (as `play` does); wrap the command in `sdk.commands.request` to get the
---error as a result instead.
---@param path string mod-relative PCM16 WAV
function sdk.audio.preload(path) end
---Start a voice under `key`, replacing any voice this mod already plays under that key.
---@param key string
---@param opts AudioPlayOptions
function sdk.audio.play(key, opts) end
---Change a playing voice; never restarts it. Unknown or finished keys are ignored. Volume and pitch
---changes are smoothed (≈ 12 ms / 25 ms).
---@param key string
---@param opts AudioUpdateOptions
function sdk.audio.update(key, opts) end
---Stop a voice with a fade (seconds, default 0.03; 0 = at once). Repeated stops don't extend it.
---@param key string
---@param fade_out? number
function sdk.audio.stop(key, fade_out) end
---Stop every voice of this mod at once (the clips stay loaded).
function sdk.audio.stop_all() end

-- Audio extension 2 (capability `audio` >= 2): the game's own native audio, the same calls engine
-- systems make. Keys belong to the calling mod: 32 handles per mod, 128 in all, 16 posts per frame.
-- Posts and globals are applied at the start of the next audio pass. Everything is released and
-- every global restored when the mod stops, fails or reloads; posts also end at a map change (the
-- handle reads `live = false`; post again on `world_changed`) and when the game's sound restarts
-- for an audio content change. A post runs the retail bank's program, which draws from the one
-- random generator every retail post uses: with a mod posting, the retail random sequence differs.
---@class AudioHandle
---@field live boolean the post is held by the running audio (false after a map change / restart)
---@field class string the class it was posted to
---@class AudioMixMapRow
---@field slot string
---@field object integer
---@field instance integer
---@field output integer
---@field level integer 0..32767 (a Q15 level, or a filter cutoff in Hz)
---@field raw integer 0..65535 (an azimuth: 65536 = 360°)
---@field pitch integer 4096 = 1.0
---@field half integer the output word as stored
---@class AudioWatchKey
---@field slot "global"|"player"|"ambience"|"collision"|"traffic"|"pedestrian"|"emitter"
---@field object? integer 0..127 (default 0)
---@field instance? integer 0..31 (default 0)
---@field output integer 0..31
---@class AudioWatchOptions
---@field globals? string[] up to 16 retail globals
---@field mixmap? AudioWatchKey[] up to 16 MixMap outputs
---@class AudioEvent
---@field kind "post"|"release"|"splice"|"emitter_start"|"emitter_stop"|"zone"|"speech"|"body_impact"
---@field source "player"|"world"|"npc"|"emitter"|"ambience"|"speech"
---@field class string retail class (posts), bank (Splice starts, emitters), "speech" / "maincast" (speech lines) or ""
---@field slot string the poster's slot (`grind`, `footstep`, `horn`, `ped_tazer`, `body_fall`, `ring`, …) or ""
---@field id integer Splice sound id, emitter patch, slot index, speech event
---@field owner string world / NPC object, zone key, speaker ("0" for the local player)
---@field tag? "pop"|"land"|"grind_start"|"grind_end"|"footstep"|"horn"|"alarm"|"tazer"|"body_fall"|"emitter"|"zone_change"|"speech"|"body_impact"
---@field region? integer body_impact: the body region (0 head, 1 torso, 2 / 3 arms, 4 / 5 legs)
---@field impact? number body_impact: the impact the body poster read (after the speed graph)
---@field tier? integer[] body_impact: the pair's tiers {body material, surface} (0..2, 3 = none)
---@field material? integer[] body_impact: {body material, surface material} (143 = none)
---@field position? number[] body_impact: {x, y, z} where the hit plays (the skater's body point)
---@class AudioInfo
---@field native boolean the native audio runtime runs
---@field map_epoch? integer
---@field generation? integer audio content generation (bumped by every content change: a swap or a restart)
---@field restarts? integer runtime restarts (only where a change cannot be swapped in place: the MixMap file, the rolling bed's grains)
---@field swaps? integer audio content changes swapped into the running audio without a restart (audio_content >= 3)
---@field last_change? string the last content change: "swap", or "restart: <reasons>"
---@field mixmap_inputs? integer MixMap inputs written by mods (audio >= 4)
---@field seed? {owner:string, seed:integer} the audio random state's seed in force (audio >= 4)
---@field overlays? string[] mods whose audio.json is applied
---@field conflicts? integer
---@field map? {stem:string, district:string, ems:string[], sources:string[]}
---@field limits table
---@field tags string[]
---Post to a retail class (e.g. `c_emitter`) with up to 32 payload words; a key's post replaces its
---last one. An unknown class is a command error (use `sdk.commands.request` to get it as a result).
---@param key string
---@param class string
---@param words? integer[]
function sdk.audio.post(key, class, words) end
---Rewrite a held post's payload.
---@param key string
---@param words integer[]
function sdk.audio.redeliver(key, words) end
---@param key string
function sdk.audio.release(key) end
---Set a retail global; `nil` restores the value seen before this mod's first write. The first mod
---to set a global owns it; it is restored when that mod stops and at a map change.
---@param name string
---@param value? integer
function sdk.audio.set_global(name, value) end
---Replace this mod's watch lists (read with `sdk.audio.global` / `sdk.audio.mixmap`).
---@param opts AudioWatchOptions
function sdk.audio.watch(opts) end
---@param key string
---@return AudioHandle|nil
function sdk.audio.handle(key) end
---A watched global's value after the last audio pass (or the value this mod set).
---@param name string
---@return integer|nil
function sdk.audio.global(name) end
---A watched MixMap output after the last audio pass.
---@return AudioMixMapRow|nil
function sdk.audio.mixmap(slot, object, instance, output) end
---@return AudioInfo
function sdk.audio.info() end
-- Audio events (capability `audio_events`): observe only, one frame late, at most 256 rows a frame.
-- Nothing is recorded while no mod subscribes.
---Subscribe to the rows with these tags (`{tags={}}` = every row); `nil` stops.
---@param opts? {tags:string[]}
function sdk.audio.subscribe(opts) end
---The rows of the last frame not returned yet (each frame's rows once).
---@return AudioEvent[]
function sdk.audio.events() end
-- Rules (capability `audio_events` >= 2): mute / replace / layer the game's own sounds where they are
-- posted, the same frame (Lua can't run in the audio pass, so rules are declarative). Sites: the local
-- player's component posts and Splice starts (pops, landings, foley), the world / NPC hosts' posts and
-- Splice starts, the world emitters' starts. `mute`: the request is dropped (a post is not made: its
-- updates and release do nothing; a Splice sound does not start; an emitter keeps its state silently);
-- `replace`: dropped + `play`; `layer`: kept + `play`. The rule's sound plays where the game's would
-- (at its owner, following it) unless `play.at` says otherwise. The first matching rule decides (mods
-- in mod-id order). Event rows still report muted requests. 32 rules per mod, 64 in all; removed when the mod
-- stops. Static rules also go in audio.json `rules` (capability audio_content >= 2).
---@class AudioRuleMatch
---@field tag? 'pop'|'land'|'grind_start'|'grind_end'|'footstep'|'horn'|'alarm'|'tazer'|'body_fall'|'emitter'
---@field kind? 'post'|'splice'|'emitter_start'
---@field source? 'player'|'world'|'npc'|'emitter'
---@field class? string a retail class (posts) or bank (Splice starts, emitters)
---@field slot? string the poster's slot (grind, wind, footstep, horn, ped_footstep, ...)
---@field id? integer slot index (posts), sound id (Splice), patch (emitters)
---@class AudioRulePlay
---@field path string mod-relative PCM16 WAV, played through the native mixer as a one-shot
---@field volume? number 0..1 (default 1)
---@field pitch? number 0.25..4 (default 1)
---@field reverb? boolean environment send (default true)
---@field group? 'player'|'world' Effects (default) or Ambience volume
---@field at? 'owner'|'world'|'centre' where it plays (default 'owner'): `owner` = at the owner of the
---replaced / layered sound (the local skater's centre of mass, the car or ped, the NPC skater, the
---emitter), following it while it plays (an owner that is gone leaves it where it was; one never found
---plays it centred); `world` = at the fixed `position`; `centre` = non-positional, centred (the retail
---non-positional emitter outputs). Positional sounds use the retail emitter law (MixMap Emitter dry
---level, pan, reverb send rolling off with camera distance) and a reach (`falloff`).
---@field offset? Vec3 `owner` only: metres added to the owner's position, -100..100 (world axes, y up; with `frame = 'owner'` the owner's axes)
---@field frame? 'world'|'owner' `owner` only (audio_events >= 3): the axes of `offset`: 'world' (default) or 'owner' = x its right, y up, z its facing (the board's nose for skaters, a car's direction, a ped's walking direction, an emitter's forward), turning with it. A rule sound at a published emitter follows it when it moves.
---@field position? Vec3 `world` only (required there): the world position
---@field falloff? AudioFalloff positional only: the reach; default the owner's retail reach: the emitter
---record's own shape and curve, 40 m for cars (retail's traffic list), 50 m for peds (the ped list),
---30 m for skaters (the local player too: retail's skater audio radius); squared curve
---@class AudioRule
---@field match AudioRuleMatch at least one field; all given fields must hold
---@field action 'mute'|'replace'|'layer'
---@field play? AudioRulePlay required for replace / layer
---@field min_interval? number seconds between two plays of the rule's sound (default 0.05)
---Set (a table) or remove (`nil`) this mod's rule `key`. A bad rule or WAV is a command error.
---@param key string
---@param rule AudioRule|nil
function sdk.audio.rule(key, rule) end
-- Audio extension 4 (capability `audio` >= 4).
---Write (or with `value = nil` release) one input (0..15) of a retail MixMap controller: drive the
---game's own controllers (the Master category gains `'global', 2, 0, 1..4`, the duck flags of the
---Global objects, an instance's inputs) through retail's curves and envelopes. Applied every audio
---pass after the game's own writes, right before the evaluations, so it holds against inputs the game
---writes too. The first mod to write an input owns it; 16 per mod, 64 in all; an unknown controller is
---a command error. Released (the value before the first write comes back) by nil, when the mod stops,
---fails or reloads; kept across map changes, written again after an audio restart.
---@param slot 'global'|'player'|'ambience'|'collision'|'traffic'|'pedestrian'|'emitter'
---@param object integer 0..127
---@param instance integer 0..31
---@param input integer 0..15
---@param value integer|number|nil an integer word (or with `opts.float` an f32, the distance inputs)
---@param opts? {float?:boolean}
function sdk.audio.set_mixmap_input(slot, object, instance, input, value, opts) end
---The MixMap inputs this mod writes.
---@return {slot:string, object:integer, instance:integer, input:integer, value:number}[]
function sdk.audio.mixmap_inputs() end
---Seed the audio random state for reproducible tests: every audio generator (programs, Splice picks,
---grain picks, eEQChain rolls, Jitter, the world and speech hosts) is set from `n` at the start of the
---next audio pass, so the same seed at the same point draws the same. One owner (the first mod);
---`nil` (or the mod stopping) puts back the states the generators had at the first seed. Unseeded the
---draws are retail's, unchanged. `SKATE_AUDIO_SEED=<n>` seeds a whole run.
---@param n integer|nil
function sdk.audio.seed(n) end
-- Audio content (capability `audio_content`): a mod ships `audio.json` at its root (no Lua needed):
-- replace / add retail audio content by identity (banks, sample slots, Splice trees, grain members,
-- wheel streams, ambience beds, emitter records, location sets, zones, crossfades, speech takes,
-- tuning fields, map audio, Csis projects) with its own files. Applied only while the mod runs.
-- Capability audio_content >= 3: turning the mod on or off, or editing audio.json / its files while
-- it runs, is swapped into the running audio in place (only what changed is replaced; held sounds
-- continue on the new content); only a MixMap or grain change restarts the game's sound (a short
-- cut). `add.projects` = the mod's own `.csi` projects (new classes, functions, globals; names must
-- not be the install's or another mod's). Two mods on one identity: the first by mod id wins
-- and the mod menu shows the conflict. Check it with `check_mod <package> --install <assets>`.
-- Reference: docs/hails-additions/16-audio-modding.md.

-- World tuning (capability `world_tuning`): typed patches while this mod runs (every field optional;
-- an absent field keeps the shipped value; first writer wins per field; restored when the mod stops,
-- fails or reloads). Domains:
-- 'living_world': npc_draw_distance (0.25..4, 1 = retail), skater_fade {fade_in_seconds, fade_seconds,
--   despawn_alpha} (retail 1, 1, 0.2), skater_line_chain {radius, max_candidates, blend_seconds, keep_facing,
--   facing_rule, steer_dead_zone_deg, steer_full_deg, fakie_high_speed, fakie_low_speed, fakie_slow_seconds,
--   fakie_spawn_seconds} (NPC skater line end:
--   continue on an unused line starting within radius m, retail 4 and 16; radius 0 = fade out at every
--   line end; blend_seconds: the drawn skater moves onto the new line over this time after a branch or
--   chain, default 0.2, 0 = cut; keep_facing: mod option, not retail, default false: the skater keeps the
--   way it faces, forward or fakie, across a branch or chain by riding the new line turned round;
--   facing_rule: 'riding_entry' (default, retail: the recorded skater frame, turned while a flip
--   latched on landing / spawn / getting on the board is set, held across switches; a body drawn
--   against its travel on the ground is drawn riding fakie with the stock fakie channel, like retail)
--   or 'per_node' (not retail: each node folded onto the board's riding direction); steer_dead_zone_deg / steer_full_deg: retail AI
--   steer ramp, 2 and 10, kept for the simulated tier, unused by the replay tier; fakie_high_speed,
--   fakie_low_speed (m/s), fakie_slow_seconds, fakie_spawn_seconds: retail's riding-fakie rule (stock
--   1, 0.5, 0.2, 1): drawn fakie when rolling against the board's forward above the high speed, or above
--   the low speed for longer than the slow time, never in the first spawn seconds; a very high speed
--   turns the fakie drawing off), ped_fade {distance = {near, far} (45, 55; a model record's pair
--   wins), fade_in_seconds (1), enabled}, skater_clips {[phase or 'phase.Style'] = stock clip name}
--   (NPC skater clip per replay phase: rolling, crouched, air, air_trick, ground_trick, off_board;
--   also 'fakie_channel' = the stock tree overlaid while riding fakie, default 'B_FAKIE_CHANNEL';
--   a clip whose name holds _CYC loops; an unknown clip falls back to the shipped pick; also
--   'trick.<trick id name>' = a stock trick animation base, e.g. ['trick.kickflip'] = 'B_HEELFLIP_IN',
--   played as <base>_G on the ground then <base>_A in the air, for every recorded trick slot of that trick),
--   skater_blend_seconds {[phase or 'default'] = seconds} (NPC skater crossfade into a phase's clip,
--   the player's graph transition curve; stock default 0.2, 0 = cut, max 10; 'trick_takeoff' 0.05 and
--   'trick_air' 0.1 set the transitions into a trick's ground and air clips),
--   ped_obstacles {enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, step_height,
--   held_is_obstacle, moving_solid} (props and mod bodies as ped navigation obstacles; retail on, 0.2, 0.4,
--   0.25, held props stay obstacles (true); ours 0.1, 0, moving objects block a ped's step (true)),
--   npc_skater_props {enabled} (NPC skaters push dynamic props like the player; retail on),
--   ped_vehicle_contact {enabled, push} (traffic cars push peds out of the way; retail on / on, no knock-down),
--   npc_tricks {mode, gate_window, min_air_frames} (the trick an NPC skater does at a recorded ollie / flip
--   slot: "profile" = retail, re-picked from the character's profile table when the recorded air lasts more
--   than min_air_frames before the landing; "recorded" = the line's own trick; "none" = no ollies / flips;
--   retail windows 300 / 50 recorded 60 Hz frames),
--   skater_trick_profiles {[<character key> or <ai_skater_profiles name>] = {regular = {{trick = <id 0..331>,
--   weight}, ...}, nollie = {...}}} (replaces that table; an absent table keeps the disc's; a character key
--   wins over a profile name),
--   npc_simulated {enabled, radius, max} (NPC skaters near the player as full physics skaters driven by their
--   AI record; default off until play-tested, 40 m, 3),
--   skaters / pedestrians / vehicles {enabled, density} (density 1 = retail, 0..4, scales the census caps),
--   ambient_skaters (NPC skaters offline, retail 3, 0..8),
--   free_play {traffic, pedestrians, ai_skaters} (retail Free Play mode: traffic / peds 0..1, 0 removes them at once;
--   ai_skaters on / off; leaving it out keeps career free roam).
--   Events: on_event {name = "living_world", event = "spawn" | "despawn" (record = kind, id, tick, position,
--   heading, seed, choice / reason), "npc_trick" (id, line, node, recorded, chosen), "npc_line_end" (id),
--   "vehicle_contact" (contact = car, ped, speeds, position, normal, depth, reaction), "ped_hit" (id, kind =
--   "Knockdown" | "Standing", direction = "FromFront" | "FromBack" | "FromLeft" | "FromRight", closing m/s)}.
-- 'props': default and by_template[<MOBJ template name>] = {contact_padding, penetration_slop,
--   penetration_correction, max_depenetration_per_tick, restitution_threshold, skater_push_mass,
--   push_transfer, body_push_speed, board_push_speed, penetration_push_speed, stuck_release_ticks,
--   collision_box = {center = {x,y,z}, half_extents = {x,y,z}}}; a template entry starts from the default.
-- 'carry': grab_bit (28, RB), placement_bit (20, B), grab_range (2.0 m), and moving a held prop:
--   push_speed (1.4 m/s), pull_speed (1.0 m/s), side_speed (0.8 m/s) at full left stick,
--   turn_rate (1.6 rad/s) at full right stick X, grip_reach (0.35 m between the skater and the
--   dragged prop's near face). Per prop type: by_template[<MOBJ template name or type record name,
--   e.g. 'dt_garbagebin' (HELD_PROP type=)>] = {material_held, material_free, material_free_upright,
--   upright_pair, restitution, record_272, linear_drag, angular_drag (per second), mass (kg),
--   maximum_linear_velocity (m/s), maximum_angular_velocity (rad/s), inertia_scale = {x,y,z},
--   inertia_offset = {x,y,z} (box inertia from the half extents x scale + offset)}; unset fields
--   keep the type's retail values (the disc's livingworld_dynamicobject_characteristics record),
--   restored when the mod is disabled.
-- 'shadows': world_floor = {r, g, b} (each 0..1; retail {0.05, 0.09, 0.13}): the lightest a dynamic
--   object's shadow (skaters, cars, props, mod graphics) can leave on the baked world. Retail adds it
--   to the shadow map and keeps the darker of that and the baked lightmap, so shadows that fall into
--   baked shade (a car on a bridge over shaded ground) leave no mark; 0 = full-strength shadows.
-- 'backdrop': visible (retail true): the district's global presentation model (Industrial's sea
--   around the docks and harbour, the far sea planes, distant tree walls); false hides it.
--   proxy_terrain (retail true): the far-proxy hills retail draws where no full-detail cell
--   replaces them (Industrial's south hills under the tree wall); false hides them.
-- 'respawn': air_timeout_ticks (retail 300, integer 1..216000): fixed 1/60 s ticks a skater may stay
--   in the air before the game sends them to the last safe checkpoint (retail: 5 s, e.g. falling off
--   the map). The wipeout auto reset keeps its own vault times.
-- 'exposure': the auto-exposure meter. meter_weights = {r, g, b} (each 0..1; retail {0.3, 0.4, 0.3}):
--   how much each channel of the tone-mapped frame counts towards its brightness; meter_scale (0..100;
--   retail 2.515): the factor on the centre-weighted average before it is compared with the area's
--   target. A higher reading lowers the exposure.
-- 'ghost': the skater fades in after every placement (checkpoint respawn, teleport, session marker
--   return, spawn). enabled (retail true); fade_in_seconds (retail 1.0, 0..60, 0 = no fade): seconds from
--   placement to fully solid; hold_alpha (retail 0.68, 0..1): the opacity retail waits at while its hold
--   condition is set (not decoded yet, so it has no effect now).
-- 'decals': opacity (retail 1.0, 0..1): strength of every world decal (stains, wear, graphics) over its
--   surface. Retail blends each decal at its own texture alpha; lower values fade them. Applies at once.
sdk.world = {}
---Set (a table) or restore (`nil`) this mod's patch of a world tuning domain.
---@param domain 'living_world'|'props'|'carry'|'shadows'|'backdrop'|'respawn'|'exposure'|'ghost'|'decals'
---@param patch table|nil
function sdk.world.set_tuning(domain, patch) end
---Request a domain as the game uses it now; read it as `sdk.commands.result(key).value`.
---@param key string command result key
---@param domain 'living_world'|'props'|'carry'|'shadows'|'backdrop'|'respawn'|'exposure'|'ghost'|'decals'
function sdk.world.tuning(key, domain) end

-- Audio tuning (capability `audio_tuning`): patch the game's typed tuning while this mod runs.
-- Domains: 'player' (player_tuning: surfaces, grinds, seams, landing / collision materials, tricks,
-- treatment, contacts), 'world' (world_tuning: traffic engine records, ped footsteps / objects,
-- speech events and voice), 'bus' (bus_tuning: reverb presets, eEQChain buses, FlangeSub returns),
-- 'reverb' (bus_tuning.reverb: preset key → its 44 values by index). A patch is a field merge
-- (tables by field, arrays by decimal index `{['3'] = 0.5}`, leaves of the same type: numbers,
-- booleans, strings, number arrays of the same length); a field the install lacks or another type is
-- a command error. Applied between audio passes; the first mod to write a field owns it (another
-- mod's write of it is an error); restored when the mod stops, fails or reloads. Kept across map
-- changes. The MixMap's layout is not tunable.
---Set (a table) or restore (`nil`) this mod's patch of a tuning domain.
---@param domain 'player'|'world'|'bus'|'reverb'
---@param patch table|nil
function sdk.audio.set_tuning(domain, patch) end
---Request a domain (or a path inside it, e.g. 'traffic_engine/c04_taxi01') as the game uses it now;
---read it as `sdk.commands.result(key).value` (at most 256 KiB: read a path for big domains).
---@param key string command result key
---@param domain 'player'|'world'|'bus'|'reverb'
---@param path? string
function sdk.audio.tuning(key, domain, path) end
---The tuning fields this mod owns, as applied at the last audio pass ('world_tuning/traffic_engine/c04_taxi01/idle_rpm', …).
---@return string[]
function sdk.audio.tuned() end

-- Front-end sounds (audio extension 1): the game's own UI sounds, retail's `fe` records by name
-- (cellphone_activate, cellphone_place_marker, cellphone_marker_error, cellphone_goto_marker,
-- challenge_count_1..3, challenge_count_go, core_a_button, ...), played as the game's UI plays them
-- (sk8_menu, the record's level, at most 10 at once). Unknown names play nothing. The session marker
-- sends on_event {name="session_marker", action="opened"|"placed"|"refused"|"returned"} to every
-- running mod (doc docs/hails-additions/15-world-audio.md "Session marker sounds").
sdk.audio = sdk.audio or {}
---@param name string
function sdk.audio.frontend(name) end

-- The teleport effect (audio extension 1): the screen static and the skater's teleport crackle
-- (retail's teleport effect amount, which the session marker's Go To Marker hold ramps 0 -> 1 over
-- 0.2-1 s; the crackle comes from the Treatments bank's program). Holds for four UI ticks (1/15 s):
-- send it every frame for as long as it should last; 0 clears it. The larger of the game's and the
-- mod's amount is used.
---@param amount number 0..1
function sdk.audio.teleport_effect(amount) end

-- World audio extension 1 (capability `world_audio`): publish traffic vehicles, pedestrians and
-- skaters to the game's retail world audio, exactly as an engine system would (doc
-- docs/hails-additions/15-world-audio.md). Keys belong to this mod; 48 objects per mod, 128 in all;
-- an object not updated for 0.5 s is parked; everything is removed on disable / reload.
-- Cars and peds (capability `world_audio` >= 4): by default each takes its own instance of a
-- private MixMap (as mod emitters do; not retail): it plays whenever it is within retail's list
-- radius (40 m cars, 50 m peds) and among the 16 nearest own cars / 16 nearest own peds of all mods;
-- a farther one waits, silent (`read(key).waiting`), and takes an instance as soon as it is among
-- the nearest; it is never refused and never takes one of retail's instances, so the map's objects
-- keep retail's pools. `slots = 'shared'` (alias 'retail') puts a car / ped in retail's pools with
-- the map's objects instead, where the game decides who is audible with retail's limits (4 nearest
-- cars within 40 m, 15 nearest peds within 50 m, footsteps for the nearest 3; 8 / 24 / 3 with the
-- opt-in non-retail "more audible" setting). Skaters: one within 30 m (retail's Player slot).
-- (`world_audio` 3 had 'retail' as the default and `slots = 'own'` as the opt-in.)
-- Extension 2 (capability `world_audio` >= 2): 'emitter' and 'reverb_zone' objects: `.ems`-style
-- records added to the map's live lists. An emitter plays an AEMS bank bound to c_emitter (a retail
-- bank or one a content overlay adds) through retail's reach test (sphere when the three extents are
-- equal, else an ellipsoid along forward / up / side; inner core at full level), falloff curve and
-- c_emitter post with the MixMap Emitter words; by default it has its own emitter instance (the map's
-- emitters keep retail's 5 emitter states; mod emitters take instances of a private MixMap with the
-- same words, up to 32 shared with native mod voices); settings/audio.json "mod_emitter_slots":
-- "shared" makes them share retail's 5 with the map's emitters instead (first reached, first served).
-- A reverb zone joins the zones the reverb selector walks, after
-- the map's. Neither parks; `read(key).audible` = playing / holding the listener.
---@class WorldAudioOptions
---@field position? Vec3 world position (ignored while body is set)
---@field velocity? Vec3 m/s; default: from the position change (give it for teleporting objects)
---@field heading? number rad about +Y, 0 = +Z
---@field body? string follow one of this mod's physics bodies (position, rotation, velocity)
---@field engine? string traffic: aud_traffic_engine record (c01_family01, c03_sports01, c04_taxi01, c05_truck01, c00_heavy01, c06_sports02, c07_family02, c08_family03) or a living-world model mapped to one as retail does (taxi01, patrol01, sedan02, hatchback01, sports03, muscle01, suv02, pickup01, minivan01, ...); unknown = silent
---@field speed? number traffic / lite skater: m/s (default |velocity|)
---@field load? number traffic: the driver's signed acceleration m/s² (default: from the speed change; hard stop ≈ -15)
---@field horn? integer traffic: 0 none, 1..5 horn kind, 6 alarm (prefer the events)
---@field skidding? boolean traffic: the tyres skid
---@field parked? boolean traffic: parked (retail's StayingParked): an impact can set its alarm off (default: parked while this mod doesn't update it, 0.5 s)
---@field voice? integer ped: the model = speech voice id 41..96 (0 none); its shoe class, kind, speech words and far threshold follow (retail's aud_characteristics). skater: its voice (AI skaters 89..96): the bail grunt
---@field shoe_class? integer ped: 1..5 (default: the model's, else 2; 1 is silent)
---@field weight? integer ped: 1..5 (default 1)
---@field close_range? boolean ped: a security guard's close-range footstep / speech levels (default: the model's)
---@field feet? boolean[] ped: {foot A planted, foot B planted}
---@field materials? integer[] ped: audio surface materials under the feet (default 0)
---@field footsteps? boolean ped: footsteps on (default: retail's 3-nearest rule)
---@field tazing? boolean ped: tazing (the c_tazer zap burst plays while it holds; prefer the tazer event)
---@field photo_flag? boolean ped: raise the game flag of the photographer's repeat (a ped holding speech value 29, PictureTaking, repeats it every second while any published ped sets this)
---@field source? string skater (spawn only): 'lite' (default) or 'state_log:<name>' (a ghost replaying logs/<name>.tsv of this mod or SKATE_AUDIO_STATE_LOGS/<name>.tsv)
---@field from? number ghost: window start (s)
---@field seconds? number ghost: window length (s, default 20), looped
---@field wheels? boolean[] lite skater: wheels down {FL, FR, RL, RR}
---@field material? integer lite skater: material under the board (default: the ground's)
---@field grinding? boolean lite skater
---@field grind_material? integer lite skater
---@field air? boolean lite skater: in the air
---@field loose_board? integer lite skater: the loose board (0 none, 1 upside down, 2 on its side): the board slide holds while set (ghosts take it from their log)
---@field bank? string emitter (spawn: required): the AEMS bank bound to c_emitter (unknown bank = command error)
---@field patch? integer emitter: the attribute patch = the c_emitter selector 0..500 (default 0)
---@field volume? number emitter: attribute volume 0..1 (default 1): level = volume × curve(d)
---@field falloff? 'squared'|'linear'|'flat' emitter: eVolumeFalloffType 0 / 1 / 2 (default 'squared')
---@field extent? Vec3 emitter / reverb_zone (spawn: required): extents in m (0.1..10000)
---@field forward? Vec3 emitter / reverb_zone: the ellipsoid's forward axis, turned by heading (default {1,0,0})
---@field core? number emitter / reverb_zone: inner core fraction 0..1 (default 0)
---@field preset? string reverb_zone (spawn: required): the aud_reverb preset key, 16 hex digits, one the install has
---@field slots? 'own'|'shared'|'retail' traffic / ped, spawn only: 'own' (the default since world_audio 4) = its own instance of a private MixMap (not retail): it plays within retail's list radius (40 m cars, 50 m peds) when among the 16 nearest own cars / 16 nearest own peds (all mods), else it waits (`read(key).waiting`); the map's objects keep retail's pools. 'shared' (alias 'retail', the world_audio 3 default) = retail's pools shared with the map's objects, the nearest win
sdk.world_audio = {}
---@param key string
---@param kind 'traffic'|'ped'|'skater'|'emitter'|'reverb_zone'
---@param opts? WorldAudioOptions
function sdk.world_audio.spawn(key, kind, opts) end
---Merge fields into the object's description (a field of another kind is an error).
---@param key string
---@param opts WorldAudioOptions
function sdk.world_audio.update(key, opts) end
---@param key string
---@param event 'horn'|'alarm'|'impact'|'speech'|'tazer'|'body_fall'|'reaction'
---@param opts? {kind?:integer, seconds?:number, value?:string|integer, by?:integer, speed?:number, source?:'player'|'character'|'vehicle'|'object'} impact (traffic): something touched the car with speed m/s (source default 'player'); the game applies retail's car alarm rule: a parked car whose contact exceeds 0.1 sets its alarm off for 8 s, every further contact restarts it, a car that is not parked ignores it (send one per contact or every frame while touching); reaction (skaters): value slam / slam_b / trick / crash / chase, by = the other skater's model (0 = the player; a pro 1..29 picks the pro-on-pro lines): the skater's own speech process says the matching line of its voice for one console frame (AI skaters 89..96 on the living-world channel, pros 1..29 / special cast 30..38 on the main cast); horn: kind 1..5 for seconds; alarm: retail's 8 s; speech: value name (warn = 53, cheer, slam, flee, nearby, DoWarning, LongCheer, ...) or number; the ped says a line of its voice through retail's speech manager (gated by the event's timers and probability) when the speech decode is installed; a repeated value re-triggers (49 rings a phone, then the ped answers); tazer: the ped zaps for seconds (default the state graph's 2 s): retail's c_tazer burst; body_fall: one BodyFallType key of a knock-down animation (kind 9, 8 or any other value 1..255: three Skate_Collisions sounds; retail's falls go 9, other, 9, 9 about 0.1 / 0.5 / 0.16 s apart)
function sdk.world_audio.event(key, event, opts) end
---@param key string
function sdk.world_audio.remove(key) end
---@param key string
---audible: holds an instance / plays; own: the instance is a private MixMap's; slots (cars, peds; world_audio >= 4): 'own' or 'shared', the object's setting; waiting (>= 4): in reach but every instance of its pool is held by a nearer object; alarm (cars): the car alarm's seconds left while it sounds.
---@return {kind:string, audible:boolean, instance?:integer, own:boolean, slots?:'own'|'shared', waiting:boolean, parked:boolean, alarm?:number}|nil
function sdk.world_audio.read(key) end
---Retail's car alarm trigger for every car (engine traffic too): the fields given replace the rule's numbers; no argument = back to retail's (contact > 0.1, 8 s; the install's setup data). Cleared when this mod stops.
---@param opts? {enabled?:boolean, min_impact?:number, seconds?:number}
function sdk.world_audio.alarm_rule(opts) end
---The retail pools (instances, published, audible, waiting) and, under `own` (world_audio >= 4), the private MixMap's (instances 16 / 16, published, audible, waiting).
---@return {more_audible:boolean, instances:{traffic:integer,peds:integer,skaters:integer}, published:table, audible:table, waiting:integer, speech_lines:integer, own:{instances:{traffic:integer,peds:integer}, published:table, audible:table, waiting:integer}}
function sdk.world_audio.info() end
