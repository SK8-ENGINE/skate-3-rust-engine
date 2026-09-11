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
---@field position Vec3
---@field velocity Vec3
---@field heading number
---@field on_board boolean
---@field state integer
---@field category integer
---@field bailing boolean
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
---@field states table<string, table<string, table<string, any>>> mod_id → peer_id → key → value
---@field status string
---@class SDKSnapshot
---@field player PlayerSnapshot
---@field map {name:string, generation:integer}
---@field tick integer
---@field keys table<string,boolean>
---@field actions number[]
---@field pad PadSnapshot
---@field paused boolean
---@field replay boolean
---@field attach? {body:string, owner:string}
---@field physics {bodies:table<string,BodySnapshot>, contacts:ContactEvent[], touching:TouchingPair[]}
---@field network? NetworkInfo
---@class ModCallbacks
---@field on_load? fun()
---@field on_unload? fun()
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
---@param opts {path?:string, body?:string, position?:Vec3, rotation?:Quat, scale?:Vec3, color?:Vec3, visible?:boolean}
function sdk.graphics.mesh(key, opts) end
---@param key string
function sdk.graphics.remove(key) end
---@param key string
---@param visible boolean
function sdk.graphics.set_visible(key, visible) end
---@return PlayerSnapshot
function sdk.player.read() end
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
---@param key string
---@param value any JSON-compatible; at most 512 encoded bytes; nil clears
function sdk.net.publish(key, value) end
---@param peer string|number peer id from info().local_id / remote peers
---@param key string
---@return any|nil
function sdk.net.read(peer, key) end
