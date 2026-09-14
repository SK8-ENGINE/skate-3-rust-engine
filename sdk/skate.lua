---@meta
-- SDK 2 language-server declarations. Not executed at runtime.
---@alias Vec3 number[]
---@alias Quat number[] xyzw
---@class BodyDesc
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
---@field position Vec3
---@field rotation Quat
---@field linvel Vec3
---@field angvel Vec3
---@field force Vec3 last sdk.physics.force this tick (zeros if none); Rapier user forces are cleared each tick
---@field torque Vec3 last sdk.physics.torque this tick
---@field mass number
---@field speed number |linvel|
---@class PlayerSnapshot
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
---@class NetworkInfo
---@field active boolean
---@field local_id string
---@field is_host boolean
---@field host_id string
---@field players string[]
---@field states table<string, table<string, table<string, any>>> mod_id → peer_id → key → value
---@field status string
---@class SDKSnapshot
---@field player PlayerSnapshot
---@field skaters table<string, PlayerSnapshot>
---@field map {name:string, generation:integer}
---@field tick integer
---@field keys table<string,boolean>
---@field actions number[]
---@field pad PadSnapshot
---@field paused boolean
---@field replay boolean
---@field camera? {position:Vec3}
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
---@param peer string|number|nil peer id to spectate, or nil to restore the native camera
function sdk.camera.watch(peer) end
---@param key string
---@return boolean
function sdk.input.down(key) end
---@param id integer
---@return number
function sdk.input.action(id) end
---@return PadSnapshot
function sdk.input.pad() end
---@param key string
---@param text string
function sdk.ui.text(key, text) end
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
---@return {tick:integer,dt:number,contacts:NativeContact[],joints:NativeJoint[],parts:table[],ragdoll:boolean,partial_ragdoll:boolean} local player only
function sdk.player.physics() end
---@return NativeContact[] most recent solved frame, max 128 reports
function sdk.player.contacts() end
---@return NativeJoint[]
function sdk.player.joints() end
---@return {index:integer,position:Vec3,velocity:Vec3,angvel:Vec3,inverse_mass:number}[]
function sdk.player.parts() end
---@param joint integer zero-based index from sdk.player.joints()
---@param options {swing_limit?:number,twist_limit?:number,free_swing?:boolean,free_twist?:boolean,drive_enabled?:boolean} angles [0.01,pi], radians
---Replaces this mod's override; omitted properties follow native state. Another mod cannot take an owned joint.
function sdk.player.set_joint(joint, options) end
---@param joint integer restores this mod's joint override
function sdk.player.reset_joint(joint) end
---Restores all joint overrides owned by this mod.
function sdk.player.reset_joints() end
