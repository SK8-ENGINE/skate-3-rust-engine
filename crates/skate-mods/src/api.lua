-- These flags come from the compiled Rust VM, NEVER from a Lua version marker.
-- An older VM loading this wrapper therefore reports an empty capability set.
sdk.capabilities = sdk._native_capabilities or {}
sdk._native_capabilities = nil

local submit = sdk._submit
sdk._submit = nil
local assets_objects = sdk._assets_objects
sdk._assets_objects = nil
local raycast_host = sdk._raycast
sdk._raycast = nil
local velocity_at_host = sdk._velocity_at
sdk._velocity_at = nil
local effective_inv_mass_host = sdk._effective_inv_mass
sdk._effective_inv_mass = nil
local spring_ray_host = sdk._spring_ray
sdk._spring_ray = nil
local local_ang_accel_impulse_host = sdk._local_ang_accel_impulse
sdk._local_ang_accel_impulse = nil

function sdk.log(text) submit{kind="log",text=text} end

sdk.ui = { version = 1 }
function sdk.ui.menu(key, options) submit{kind="ui_menu",key=key,options=options} end
function sdk.ui.remove_menu(key) submit{kind="ui_remove_menu",key=key} end
function sdk.ui.text(key, text) submit{kind="overlay",key=key,text=text} end
function sdk.ui.multiplayer_debug(key, text) submit{kind="multiplayer_debug",key=key,text=text} end
-- Persistent screen-space rectangles and text; update an existing key in place.
function sdk.ui.canvas(key, options)
    options = options or {}
    -- An empty Lua table is not a JSON array. Omit it to use the native default.
    local copy = {}; for k,v in pairs(options) do copy[k]=v end
    if type(copy.items)=="table" and next(copy.items)==nil then copy.items=nil end
    submit{kind="ui_canvas",key=key,options=copy}
end
function sdk.ui.remove(key) submit{kind="ui_remove",key=key} end

sdk.physics = {}
function sdk.physics.spawn(key, body) submit{kind="physics_spawn",key=key,body=body} end
function sdk.physics.remove(key) submit{kind="physics_remove",key=key} end
function sdk.physics.force(key, force, point) submit{kind="physics_force",key=key,force=force,point=point} end
function sdk.physics.impulse(key, impulse, point) submit{kind="physics_impulse",key=key,impulse=impulse,point=point} end
function sdk.physics.torque(key, torque) submit{kind="physics_torque",key=key,torque=torque} end
function sdk.physics.torque_impulse(key, torque) submit{kind="physics_torque_impulse",key=key,torque=torque} end
function sdk.physics.set_linvel(key, linvel) submit{kind="physics_set_linvel",key=key,linvel=linvel} end
function sdk.physics.set_angvel(key, angvel) submit{kind="physics_set_angvel",key=key,angvel=angvel} end
function sdk.physics.set_pose(key, position, rotation) submit{kind="physics_set_pose",key=key,position=position,rotation=rotation} end
function sdk.physics.revolute(key, joint)
    submit{
        kind="physics_revolute",
        key=key,
        body_a=joint.body_a,
        body_b=joint.body_b,
        anchor_a=joint.anchor_a or {0,0,0},
        anchor_b=joint.anchor_b or {0,0,0},
        axis=joint.axis or {0,1,0},
        limits=joint.limits,
        contacts_enabled=joint.contacts_enabled ~= false,
    }
end
function sdk.physics.joint_motor(key, motor) submit{kind="physics_joint_motor",key=key,motor=motor} end
function sdk.physics.prismatic(key, joint)
    submit{
        kind="physics_prismatic",
        key=key,
        body_a=joint.body_a,
        body_b=joint.body_b,
        anchor_a=joint.anchor_a or {0,0,0},
        anchor_b=joint.anchor_b or {0,0,0},
        axis=joint.axis or {0,-1,0},
        limits=joint.limits,
        contacts_enabled=joint.contacts_enabled ~= false,
    }
end
function sdk.physics.joint_spring(key, spring)
    submit{
        kind="physics_joint_spring",
        key=key,
        spring={
            target_position=spring.position or spring.target_position or 0,
            stiffness=spring.stiffness,
            damping=spring.damping,
            max_force=spring.max_force,
        },
    }
end
function sdk.physics.remove_joint(key) submit{kind="physics_remove_joint",key=key} end
function sdk.physics.add_collider(key, opts)
    opts = opts or {}
    submit{
        kind="physics_add_collider",
        key=key,
        shape=opts.shape,
        position=opts.position or {0,0,0},
        friction=opts.friction or 0.7,
    }
end
-- JSON null reaches Lua as truthy userdata unless the host maps it to nil first.
local function as_table(v) if type(v) == 'table' then return v end return nil end
local function as_number(v) if type(v) == 'number' then return v end return nil end
function sdk.physics.read(key)
    local owned = (as_table(sdk.snapshot.physics) or {}).bodies or {}
    if type(owned) ~= 'table' then return nil end
    return owned[key]
end
--- Sync raycast. opts.filter is "all" (default) or "ground".
function sdk.physics.raycast(origin, direction, opts)
    opts = opts or {}
    return as_table(raycast_host(origin, direction, {
        max_distance=opts.max_distance or 100,
        filter=opts.filter or "all",
        exclude=opts.exclude or {},
    }))
end
function sdk.physics.velocity_at(key, point)
    return as_table(velocity_at_host(key, point))
end
function sdk.physics.effective_inv_mass(key, point, direction)
    return as_number(effective_inv_mass_host(key, point, direction))
end
--- Bullet/Rapier ray spring-damper. Applies impulse immediately; returns load/contact.
function sdk.physics.spring_ray(key, opts)
    opts = opts or {}
    return as_table(spring_ray_host(key, {
        local_origin = opts.local_origin or {0,0,0},
        local_direction = opts.local_direction or {0,-1,0},
        rest_length = opts.rest_length or 0.25,
        max_travel = opts.max_travel or opts.rest_length or 0.25,
        contact_radius = opts.contact_radius or 0,
        stiffness = opts.stiffness or 30,
        compression = opts.compression or opts.damping or 4,
        relaxation = opts.relaxation or opts.damping or 4,
        max_force = opts.max_force or 6000,
        dt = opts.dt or (1/120),
    }))
end
--- World torque impulse for body-local angular acceleration over dt (assists).
function sdk.physics.local_ang_accel_impulse(key, local_accel, dt)
    return as_table(local_ang_accel_impulse_host(key, local_accel, dt or (1/120)))
end
function sdk.physics.contacts()
    return (as_table(sdk.snapshot.physics) or {}).contacts or {}
end
function sdk.physics.touching()
    return (as_table(sdk.snapshot.physics) or {}).touching or {}
end

sdk.graphics = { version = 4 }
-- Root is BODY-LOCAL when bound, WORLD when unbound. Values replace rather
-- than accumulate. Quaternions use {x,y,z,w}; angles/rates are radians.
function sdk.graphics.set_transform(key, options)
    submit{kind="graphics_transform",key=key,options=options or {}}
end
-- Named nodes are scoped to this mesh instance. Defaults to authored-pose
-- deltas; relative=false explicitly selects an absolute parent-local pose.
function sdk.graphics.node_transform(key, node, options)
    submit{kind="graphics_node",key=key,node=node,options=options or {}}
end
function sdk.graphics.reset_node(key, node)
    submit{kind="graphics_reset_node",key=key,node=node}
end
function sdk.physics.debug_colliders(enabled)
    submit{kind="physics_debug",enabled=enabled == true}
end
function sdk.graphics.mesh(key, opts)
    opts = opts or {}
    submit{
        kind="graphics_mesh",
        deform_nodes=(type(opts.deform_nodes)=="table" and next(opts.deform_nodes)) and opts.deform_nodes or nil,
        key=key,
        path=opts.path or "",
        body=opts.body,
        position=opts.position,
        rotation=opts.rotation,
        scale=opts.scale or {1,1,1},
        color=opts.color or {0.85,0.85,0.9},
        opacity=opts.opacity or 1,
        visible=opts.visible ~= false,
    }
end
function sdk.graphics.remove(key) submit{kind="graphics_remove",key=key} end
function sdk.graphics.set_visible(key, visible) submit{kind="graphics_visibility",key=key,visible=visible and true or false} end
-- Extension 3: procedural mesh buffers and scene lights. No gameplay semantics.
function sdk.graphics.mesh_buffer(key, opts)
    opts = opts or {}
    submit{kind="graphics_mesh_buffer",key=key,options={
        body=opts.body, position=opts.position, rotation=opts.rotation,
        scale=opts.scale or {1,1,1},
        blend=opts.blend ~= false, unlit=opts.unlit ~= false,
        visible=opts.visible ~= false,
        depth_bias=opts.depth_bias or 0,
        texture=opts.texture, capture=opts.capture,
        tint=opts.tint or {1,1,1},
    }}
end
local function mesh_buffer_payload(data)
    data = data or {}
    -- Empty Lua tables are maps, not arrays. Build a clean payload and omit empties.
    local payload = {}
    if data.positions and #data.positions > 0 then
        local positions = {}
        for _, p in ipairs(data.positions) do
            positions[#positions+1] = {p[1], p[2], p[3]}
        end
        payload.positions = positions
    end
    if data.indices and #data.indices > 0 then
        local indices = {}
        for _, idx in ipairs(data.indices) do
            indices[#indices+1] = math.max(0, math.floor(idx))
        end
        payload.indices = indices
    end
    if data.uvs and #data.uvs > 0 then
        local uvs = {}
        if type(data.uvs[1]) == "table" then
            for _, uv in ipairs(data.uvs) do
                uvs[#uvs+1] = uv[1]; uvs[#uvs+1] = uv[2]
            end
        else
            for _, v in ipairs(data.uvs) do uvs[#uvs+1] = v end
        end
        if #uvs > 0 then payload.uvs = uvs end
    end
    if data.colors and #data.colors > 0 then
        local colors = {}
        if type(data.colors[1]) == "table" then
            for _, c in ipairs(data.colors) do
                colors[#colors+1] = c[1]; colors[#colors+1] = c[2]
                colors[#colors+1] = c[3]; colors[#colors+1] = c[4]
            end
        else
            for _, v in ipairs(data.colors) do colors[#colors+1] = v end
        end
        if #colors > 0 then payload.colors = colors end
    end
    if data.normals and #data.normals > 0 then
        local normals = {}
        for _, n in ipairs(data.normals) do
            normals[#normals+1] = {n[1], n[2], n[3]}
        end
        payload.normals = normals
    end
    return payload
end
function sdk.graphics.mesh_buffer_write(key, data)
    submit{kind="graphics_mesh_buffer_write",key=key,data=mesh_buffer_payload(data)}
end
function sdk.graphics.mesh_buffer_append(key, data)
    submit{kind="graphics_mesh_buffer_append",key=key,data=mesh_buffer_payload(data)}
end
function sdk.graphics.light(key, opts)
    opts = opts or {}
    submit{kind="graphics_light",key=key,options={
        kind=opts.kind or "point", body=opts.body, position=opts.position,
        offset=opts.offset or {0,0,0}, direction=opts.direction,
        color=opts.color or {1,1,1},
        intensity=opts.intensity or 1000, range=opts.range or 10,
        inner_angle=opts.inner_angle or 0.4, outer_angle=opts.outer_angle or 0.7,
    }}
end

-- Audio extension 1 (backward-compatible with API 2).
-- Keys and assets are scoped to the calling mod. WAV: PCM16, mono/stereo.
-- play replaces the same key; update NEVER restarts playback.
sdk.audio = { version = 1 }
function sdk.audio.preload(path) submit{kind="audio_preload",path=path} end
function sdk.audio.play(key, opts)
    opts = opts or {}
    submit{kind="audio_play",key=key,options={
        path=opts.path, body=opts.body, position=opts.position,
        offset=opts.offset or {0,0,0}, loop=opts.loop == true,
        volume=opts.volume or 1, pitch=opts.pitch or 1,
        spatial=opts.spatial ~= false, spatial_scale=opts.spatial_scale or 0.1,
        paused=opts.paused == true, fade_in=opts.fade_in or 0.01,
        -- Audio extension 3 (capability audio >= 3): the game's native mixer, the default (nil);
        -- native = false keeps the Bevy voice, native = true requires the native mixer.
        native=opts.native, falloff=opts.falloff, reverb=opts.reverb, group=opts.group,
    }}
end
function sdk.audio.update(key, opts)
    opts = opts or {}
    submit{kind="audio_update",key=key,options={
        volume=opts.volume,pitch=opts.pitch,paused=opts.paused,
        position=opts.position,offset=opts.offset,
    }}
end
function sdk.audio.stop(key, fade_out)
    submit{kind="audio_stop",key=key,fade_out=fade_out or 0.03}
end
function sdk.audio.stop_all() submit{kind="audio_stop_all"} end
-- Audio extension 2 (capability audio >= 2): the game's own native audio. Keys are scoped to the
-- calling mod (32 handles per mod, 128 in all, 16 posts per frame); everything is released and
-- every global restored when the mod stops, fails or reloads, and posts end at a map change.
sdk.audio.version = 4
-- Post to a retail class (e.g. 'c_emitter') with up to 32 payload words; a key's post replaces
-- its last one. Applied at the start of the next audio pass.
function sdk.audio.post(key, class, words) submit{kind="audio_post",key=key,class=class,words=words or {}} end
function sdk.audio.redeliver(key, words) submit{kind="audio_redeliver",key=key,words=words or {}} end
function sdk.audio.release(key) submit{kind="audio_release",key=key} end
-- value nil restores the value seen before this mod's first write. The first mod to set a global owns it.
function sdk.audio.set_global(name, value) submit{kind="audio_set_global",name=name,value=value} end
-- Replace this mod's watch lists: {globals={'name',...}, mixmap={{slot='player', object=0, instance=0, output=4},...}}.
function sdk.audio.watch(opts)
    opts = opts or {}
    submit{kind="audio_watch",globals=opts.globals or {},mixmap=opts.mixmap or {}}
end
local function audio_mine() return as_table((as_table(sdk.snapshot.audio) or {})[sdk.mod_id]) or {} end
-- {live=bool, class=name} for one of this mod's posts (live false after a map change or an audio restart).
function sdk.audio.handle(key) return ((audio_mine().handles) or {})[key] end
-- A watched global's value after the last audio pass (or the value this mod set).
function sdk.audio.global(name)
    local m = audio_mine()
    local v = ((m.watch or {}).globals or {})[name]
    if v == nil then v = (m.set_globals or {})[name] end
    return v
end
-- A watched MixMap output after the last pass: {level=0..32767, raw=0..65535, pitch=4096 = 1.0, half=raw word}.
function sdk.audio.mixmap(slot, object, instance, output)
    for _, row in ipairs(((audio_mine().watch) or {}).mixmap or {}) do
        if row.slot == slot and row.object == (object or 0) and row.instance == (instance or 0) and row.output == output then return row end
    end
    return nil
end
-- Audio events (capability audio_events): subscribe{tags={'pop','land',...}} (empty = every row),
-- subscribe(nil) stops. Tags: pop, land, grind_start, grind_end, footstep, horn, alarm, tazer,
-- body_fall, emitter, zone_change, speech, body_impact (a skater's body region hit: region, impact, tier,
-- material, position). Observe only, one frame late; at most 256 rows a frame.
function sdk.audio.subscribe(opts)
    if opts == nil then submit{kind="audio_subscribe"} else submit{kind="audio_subscribe",tags=opts.tags or {}} end
end
local audio_events_serial = nil
-- The rows of the last frame not returned yet: {kind, source, class, slot, id, owner, tag}.
function sdk.audio.events()
    local e = as_table(audio_mine().events)
    if not e or e.serial == audio_events_serial then return {} end
    audio_events_serial = e.serial
    return as_table(e.rows) or {}
end
-- {native=bool, map_epoch, generation, restarts, overlays={ids}, conflicts, map={stem, district, ems, sources}, limits, tags}
function sdk.audio.info() return as_table(sdk.snapshot.audio_info) or {native=false} end
-- Rules (capability audio_events >= 2): mute / replace / layer the game's own sounds at their post
-- sites, the same frame: rule(key, {match={tag="pop"}, action="replace", play={path="pop.wav"}});
-- rule(key, nil) removes it. Removed when the mod stops.
function sdk.audio.rule(key, rule) submit{kind="audio_rule",key=key,rule=rule} end
-- Audio extension 4 (capability audio >= 4). MixMap inputs: drive one input (0..15) of a retail
-- MixMap controller (slot name, object, instance; e.g. 'global', 2, 0 = Master) with an integer
-- word (opts.float = true: an f32 input); nil releases it (the input's value before the first
-- write comes back). The first mod to write an input owns it; 16 per mod, 64 in all.
function sdk.audio.set_mixmap_input(slot, object, instance, input, value, opts)
    submit{kind="audio_set_mixmap_input",slot=slot,object=object,instance=instance,input=input,value=value,float=(opts and opts.float) or false}
end
-- The inputs this mod writes: {{slot=, object=, instance=, input=, value=}, ...}.
function sdk.audio.mixmap_inputs() return (audio_mine().inputs) or {} end
-- Seed the audio random state (every audio generator, from the next audio pass) for reproducible
-- tests; nil releases it (the generators get back their states from the first seed). One owner.
function sdk.audio.seed(n) submit{kind="audio_seed",seed=n} end
-- Tuning writes (capability audio_tuning): patch a typed tuning domain ("player", "world", "bus",
-- "reverb") while this mod runs; applied between audio passes; nil restores this mod's patch of
-- the domain (everything is restored when the mod stops). The first mod to write a field owns it.
function sdk.audio.set_tuning(domain, patch) submit{kind="audio_set_tuning",domain=domain,patch=patch} end
-- Read a domain (or a path inside it, "traffic_engine/c04_taxi01") as the game uses it now:
-- the value arrives as sdk.commands.result(key).value.
function sdk.audio.tuning(key, domain, path)
    sdk.engine.inspect(key, "audio_tuning:" .. domain .. (path and ("/" .. path) or ""))
end
-- The tuning fields this mod owns, as applied at the last audio pass ("world_tuning/traffic_engine/...").
function sdk.audio.tuned() return (audio_mine().tuning) or {} end
-- The game's own front-end sounds (retail `fe` records by name, played as the game's UI plays them).
function sdk.audio.frontend(name) submit{kind="audio_frontend",name=name} end
-- The teleport effect (screen static + the skater's teleport crackle) at amount 0..1; send it every frame to hold it.
function sdk.audio.teleport_effect(amount) submit{kind="audio_teleport_effect",amount=amount} end

-- World tuning (capability world_tuning): patch the living world ("living_world": npc_draw_distance,
-- skater_fade, skater_line_chain, ped_fade, skater_clips, skater_blend_seconds, skater_stance, skater_stance_events, ped_obstacles, npc_skater_props, ped_vehicle_contact, npc_tricks, skater_trick_profiles, npc_simulated, skaters, pedestrians, vehicles, ambient_skaters, free_play), dynamic props ("props": default / by_template prop tuning, collision_box)
-- or prop carrying ("carry": grab_bit, placement_bit, grab_range, push_speed, pull_speed, side_speed,
-- turn_rate, grip_reach, linear_clamp, yaw_clamp, relatch, slew_per_tick, yaw_rate_feedback, linear_controller, yaw_controller,
-- lever_rotation, lever_yaw, mass_speed, inertia_yaw_gain, let_go_distance, drop_board, follow_step, hold_angle_limit, hold_max_angle_to_horizontal, hold_box_extents,
-- record_272_speed_scale, grab_end_exclusion, hand_ik_enter, hand_ik_curve, hand_ik_rate, hand_ik_reach, commanded_material, upright_cos, apply_at_com,
-- yaw_replaces_torque, ignore_vertical, wake_on_command, by_template[<MOBJ template>] = {material_held, material_free,
-- material_free_upright, upright_pair, restitution, record_272, linear_drag, angular_drag, mass,
-- maximum_linear_velocity, maximum_angular_velocity, inertia_scale, inertia_offset})
-- or the dynamic shadow floor on the baked world ("shadows": world_floor =
-- {r, g, b}, each 0..1, retail {0.05, 0.09, 0.13}) or the district backdrop ("backdrop": visible,
-- retail true: Industrial's sea, far sea planes, tree walls; proxy_terrain, retail true: the far-proxy hills
-- under Industrial's south tree wall) or the checkpoint respawn ("respawn": air_timeout_ticks, retail 300 =
-- 5 s of 1/60 s ticks in the air before the skater is sent to the last checkpoint, 1..216000) or the
-- auto-exposure meter ("exposure": meter_weights, retail {0.3, 0.4, 0.3}; meter_scale, retail 2.515) or the skater
-- fade-in after every placement ("ghost": enabled, retail true; fade_in_seconds, retail 1.0, 0..60; hold_alpha,
-- retail 0.68, 0..1) or the world decals ("decals": opacity, retail 1.0, 0..1: strength of every decal over
-- its surface, applied at once) while this mod
-- runs; nil restores
-- this mod's patch of the domain, everything is restored when the mod stops. First writer wins.
sdk.world = { version = 1 }
function sdk.world.set_tuning(domain, patch) submit{kind="world_set_tuning",domain=domain,patch=patch} end
-- Read a domain as the game uses it now: the value arrives as sdk.commands.result(key).value.
function sdk.world.tuning(key, domain) sdk.engine.inspect(key, "world_tuning:" .. domain) end
-- Reset one dynamic prop (stable map id) to its authored pose, at rest; its saved layout entry is
-- dropped (retail cMsgResetDMO; refused for the held prop). doc 27, Object Dropper and reset.
function sdk.world.reset_prop(id) submit{kind="world_reset_prop",id=id} end
-- Upright one prop (retail phone Upright, cMsgUprightDMO): 2 s self-righting window.
function sdk.world.upright_prop(id) submit{kind="world_upright_prop",id=id} end
-- Convenience (not a retail action): reset_prop for every moved or placed prop.
function sdk.world.reset_moved_props() submit{kind="world_reset_moved_props"} end
-- Create a dynamic prop mid-game: a copy of map prop `from` (stable map id: model, collision,
-- physics) at opts.position {x, y, z}, turned opts.yaw radians, thrown at opts.velocity {x, y, z}
-- m/s with opts.spin {x, y, z} rad/s (arrays). The same key again replaces it; the mod's props go on disable.
-- doc 27, Props created mid-game.
function sdk.world.spawn_prop(key, from, opts)
  opts = opts or {}
  submit{kind="world_spawn_prop",key=key,from=from,position=opts.position,yaw=opts.yaw,velocity=opts.velocity,spin=opts.spin}
end
function sdk.world.remove_prop(key) submit{kind="world_remove_prop",key=key} end

-- World audio extension 1 (backward-compatible with API 2): publish traffic vehicles, peds and
-- skaters to the game's retail world audio (the same path engine systems use). Keys are scoped
-- to the calling mod; 48 objects per mod, 128 in all; an object not updated for 0.5 s is parked;
-- everything is removed when the mod is disabled or reloaded. The retail limits decide which
-- objects sound (4 nearest cars within 40 m, 15 nearest peds within 50 m, 1 skater within 30 m).
-- Version 4: a mod's cars and peds take their own MixMap instance by default (16 + 16, the nearest
-- own ones play, the rest wait); `slots = 'shared'` puts one in retail's pools instead.
sdk.world_audio = { version = 4 }
function sdk.world_audio.spawn(key, kind, opts)
    submit{kind="world_audio_spawn",key=key,object=kind,options=opts or {}}
end
function sdk.world_audio.update(key, opts)
    submit{kind="world_audio_update",key=key,options=opts or {}}
end
-- event: 'horn' {kind=1..5, seconds=s}, 'alarm' (retail's 8 s), 'speech' {value=name or number},
-- 'impact' {speed=m/s, source='player'|'character'|'vehicle'|'object'}: something touched this car; a parked
-- car (option parked=true, or not updated for 0.5 s) sets its alarm off as retail does (contact > 0.1, 8 s,
-- every further contact restarts it)
function sdk.world_audio.event(key, event, opts)
    submit{kind="world_audio_event",key=key,event=event,options=opts or {}}
end
function sdk.world_audio.remove(key) submit{kind="world_audio_remove",key=key} end
-- The announcer channel (retail's contest commentator, models 35 / 36). Free skate has no announcer
-- (retail: a pro's crash near the camera asks for 480_slam_pro and finds no line); naming one makes
-- those requests speak. announcer(nil) clears it (also when the mod stops).
function sdk.world_audio.announcer(character)
    submit{kind="world_audio_announcer",character=character}
end
-- event: an announcer event id (24576..24751) or name ('480_slam_pro', '422_slam', '480');
-- opts: {pro=model (its announcer pro id fills word 2), words={...} (the request block from word 0)}.
function sdk.world_audio.announce(event, opts)
    submit{kind="world_audio_announce",event=event,options=opts or {}}
end
-- Retail's car alarm trigger: opts {enabled=bool, min_impact=m/s, seconds=s} replace the rule's numbers for
-- every car (engine traffic too); alarm_rule() goes back to retail's. Cleared when the mod stops.
function sdk.world_audio.alarm_rule(opts)
    submit{kind="world_audio_alarm_rule",options=opts}
end
-- {kind=..., audible=bool, instance=n or nil, parked=bool, alarm=seconds left or nil} for one of this
-- mod's objects (nil if unknown).
function sdk.world_audio.read(key)
    local owners = as_table(sdk.snapshot.world_audio) or {}
    return (owners[sdk.mod_id] or {})[key]
end
-- {more_audible=bool, instances={traffic=4, peds=15, skaters=1}, published={...}}
function sdk.world_audio.info()
    return as_table(sdk.snapshot.world_audio_info) or {more_audible=false,instances={traffic=4,peds=15,skaters=1}}
end

sdk.player = {}
function sdk.player.suspend(suspended) submit{kind="player_suspend",suspended=suspended} end
function sdk.player.physics() return as_table(sdk.snapshot.player_physics) or {} end
function sdk.player.contacts() return sdk.player.physics().contacts or {} end
function sdk.player.joints() return sdk.player.physics().joints or {} end
function sdk.player.parts() return sdk.player.physics().parts or {} end
function sdk.player.set_joint(joint, options) submit{kind="player_joint",joint=joint,options=options} end
function sdk.player.reset_joint(joint) submit{kind="player_reset_joint",joint=joint} end
function sdk.player.reset_joints() submit{kind="player_reset_joints"} end
function sdk.player.teleport(options) submit{kind="player_teleport",options=options} end
function sdk.player.read() return as_table(sdk.snapshot.player) or {} end
function sdk.player.skaters() return as_table(sdk.snapshot.skaters) or {} end
function sdk.player.skater(id)
    local all = sdk.player.skaters()
    return all[tostring(id)]
end
function sdk.player.attach(body, offset) submit{kind="player_attach",body=body,offset=offset or {0,0,0}} end
function sdk.player.detach(options) submit{kind="player_detach",options=options or {}} end
function sdk.player.detach_error() return sdk.snapshot.detach_error end
function sdk.player.detaching() return sdk.snapshot.detach_pending == true end
function sdk.player.attached()
    local a = as_table(sdk.snapshot.attach)
    return a and a.body or nil
end

sdk.camera = { version = 2 }
-- Persistent, render-rate camera. Does not write the body's pose or velocity.
function sdk.camera.rig(body, options)
    submit{kind="camera_rig",body=body,options=options or {}}
end
function sdk.camera.clear() sdk.camera.clear_follow() end
function sdk.camera.follow(body, offset) submit{kind="camera_follow",body=body,offset=offset or {0,2.5,-6}} end
function sdk.camera.clear_follow() submit{kind="camera_follow",body=nil,offset={0,2.5,-6}} end
function sdk.camera.set(position, look_at) submit{kind="camera_set",position=position,look_at=look_at} end
-- Lock the gameplay camera onto a multiplayer skater. nil restores the native camera.
function sdk.camera.watch(peer) submit{kind="camera_watch",peer=peer ~= nil and tostring(peer) or nil} end
function sdk.camera.capture(key, options) submit{kind="camera_capture",key=key,options=options} end
function sdk.camera.clear_capture(key) submit{kind="camera_clear_capture",key=key} end
-- Retail Camera Angle (Game Settings > Control Settings): "low" or "high" (capability camera >= 4).
-- read() = {selected = player's setting, active = what the camera graph uses, owner = mod forcing it or nil,
--           shot = current stock shot, tuned = {shot = owner}}.
function sdk.camera.angle() return as_table(sdk.snapshot.camera_angle) or {} end
-- Force "low" / "high"; nil returns to the player's setting. One mod at a time; released on disable.
function sdk.camera.set_angle(angle) submit{kind="camera_angle",angle=angle} end
-- Replace stock values of one camera shot by retail attribute name (PositionDistance, PositionElevation,
-- FramingPitch, ...); nil restores the stock shot. One mod per shot; released on disable.
function sdk.camera.tune_shot(shot, patch) submit{kind="camera_shot_tune",shot=shot,patch=patch} end

sdk.session = {}
function sdk.session.info() return as_table(sdk.snapshot.session) or {} end
function sdk.session.claim() submit{kind="session_claim"} end
function sdk.session.transfer(peer) submit{kind="session_transfer",peer=tostring(peer)} end
function sdk.session.teleport(peer, options) submit{kind="session_teleport",peer=tostring(peer),options=options} end

sdk.volumes = {}
function sdk.volumes.box(key, options) submit{kind="volume_box",key=key,options=options} end
function sdk.volumes.remove(key) submit{kind="volume_remove",key=key} end
function sdk.volumes.read(key)
    local owners = as_table(sdk.snapshot.volumes) or {}
    return (owners[sdk.mod_id] or {})[key]
end

-- Named trigger volumes (retail map volumes, custom-map volumes, mod volumes).
-- Events arrive in on_event: {name="trigger_entered"|"trigger_exited", body=..., volume=...}.
sdk.triggers = { version = 1 }
local function trigger_snapshot() return as_table(sdk.snapshot.triggers) or {} end
function sdk.triggers.list() return as_table(trigger_snapshot().volumes) or {} end
function sdk.triggers.get(id)
    for _, v in ipairs(sdk.triggers.list()) do
        if v.id == id or v.name == id then return v end
    end
end
function sdk.triggers.inside(body)
    return (as_table(trigger_snapshot().bodies) or {})[body or "player"] or {}
end
function sdk.triggers.box(key, options) submit{kind="trigger_box",key=key,options=options} end
function sdk.triggers.remove(key) submit{kind="trigger_remove",key=key} end
function sdk.triggers.set_enabled(id, enabled) submit{kind="trigger_enable",id=id,enabled=enabled ~= false} end
local function non_empty(t) if type(t) == "table" and next(t) ~= nil then return t end return nil end
function sdk.triggers.track(key, options) submit{kind="trigger_track",key=key,options=non_empty(options)} end
function sdk.triggers.untrack(key) submit{kind="trigger_untrack",key=key} end
function sdk.triggers.configure(options) submit{kind="trigger_configure",options=non_empty(options)} end

sdk.input = {}
-- Mapped gameplay action IDs by stable key (retail input.cfg GP_* order, see
-- sdk/GENERAL_API.md). Accepted wherever an action ID is.
sdk.input.action_ids = {
    left_stick_x=64, left_stick_y=65, left_stick_click=66,
    right_stick_x=67, right_stick_y=68, right_stick_click=69,
    left_trigger=70, right_trigger=71, left_bumper=72, right_bumper=73,
    dpad_up=74, dpad_down=75, dpad_left=76, dpad_right=77,
    x=78, y=79, a=80, b=81,
}
local function action_id(id)
    if type(id)=='string' then
        local mapped = sdk.input.action_ids[id]
        assert(mapped~=nil,'unknown action key '..id)
        return mapped
    end
    assert(type(id)=='number' and id%1==0 and id>=64 and id<=81,'action ID must be 64..81')
    return id
end
function sdk.input.down(key)
    local keys = sdk.snapshot.keys
    return keys ~= nil and keys[key] == true
end
-- Identity of the controller in slot 0..3 (default: the slot gameplay reads),
-- or nil when the slot is empty. Read-only.
function sdk.input.controller(slot)
    local c = as_table(sdk.snapshot.controllers) or {}
    if slot == nil then slot = c.active end
    if slot == nil then return nil end
    assert(type(slot)=='number' and slot%1==0 and slot>=0 and slot<=3,'controller slot must be 0..3')
    return (as_table(c.slots) or {})[slot+1]
end
function sdk.input.controllers()
    local c = as_table(sdk.snapshot.controllers) or {}
    local slots = as_table(c.slots) or {}
    return {active=c.active, slots={slots[1],slots[2],slots[3],slots[4]}}
end
function sdk.input.action(id)
    id = action_id(id)
    local actions = sdk.snapshot.actions
    return actions and actions[id-63] or 0.0
end
function sdk.input.pad()
    return as_table(sdk.snapshot.pad) or {buttons=0,triggers={0,0},left={0,0},right={0,0}}
end

sdk.assets = {}
function sdk.assets.objects(path)
    return assets_objects(path)
end

sdk.net = {}
function sdk.net.info()
  return as_table(sdk.snapshot.network) or {active=false,local_id="0",is_host=true,host_id="0",players={"0"},states={},status=""}
end
function sdk.net.publish(key,value) submit{kind="network_state",key=key,value=value} end
function sdk.net.read(peer,key)
  local n = as_table(sdk.snapshot.network) or {}
  return ((((n.states or {})[sdk.mod_id] or {})[tostring(peer)]) or {})[key]
end
function sdk.net.players()
  local n = sdk.net.info()
  return n.players or {n.local_id or "0"}
end

sdk.time = { elapsed = 0 }
local timers = {}
function sdk.time.after(key, seconds, callback)
    assert(type(key)=='string' and #key>0 and #key<=64,'invalid timer key')
    assert(type(seconds)=='number' and seconds>=0 and seconds<=86400,'invalid timer delay')
    assert(type(callback)=='function','timer callback must be a function')
    local count=0; for _ in pairs(timers) do count=count+1 end
    assert(timers[key] or count<64,'64 timers maximum')
    timers[key]={at=sdk.time.elapsed+seconds,callback=callback}
end
function sdk.time.cancel(key) timers[key]=nil end
function sdk._timers_due(dt)
    local at=sdk.time.elapsed+dt
    for _,timer in pairs(timers) do if timer.at<=at then return true end end
    return false
end
function sdk._advance(dt)
    sdk.time.elapsed=sdk.time.elapsed+dt
    if next(timers)==nil then return end
    local due={}
    for key,timer in pairs(timers) do if timer.at<=sdk.time.elapsed then due[#due+1]=key end end
    table.sort(due)
    for _,key in ipairs(due) do
        local timer=timers[key]
        if timer and timer.at<=sdk.time.elapsed then timers[key]=nil; timer.callback() end
    end
end

-- Unified low-level surface. All native IDs below are zero-based; Lua lists are not.
sdk.commands = {}
local command_serial, command_pending = 0, {}
function sdk.commands.request(key, command)
    command_serial=command_serial+1;command_pending[key]=command_serial
    submit{kind="request",key=key,token=command_serial,command=command}
    return command_serial
end
function sdk.commands.result(key)
    local result=((sdk.snapshot.command_results or {})[sdk.mod_id] or {})[key]
    if result and result.token==command_pending[key] then return result end
end
sdk.engine = {version=1}
function sdk.engine.systems() return (sdk.snapshot.engine or {}).systems or {} end
function sdk.engine.inspect(key,system) sdk.commands.request(key,{kind="engine_inspect",system=system}) end
function sdk.engine.read(system)
    local s=sdk.snapshot
    if system=="input" then return {pad=s.pad,actions=s.actions,keys=s.keys,controllers=s.controllers} end
    if system=="commands" then return (s.command_results or {})[sdk.mod_id] end
    local key=({player="player",rig="player_physics",bodies="physics",world="map",camera="camera",network="network",scoring="player"})[system]
    if key then return s[key] end
    return (s.engine or {})[system]
end
sdk.rig = {}
function sdk.rig.read(fields)
  if fields and sdk._rig_snapshot then return sdk._rig_snapshot(fields) end
  return sdk.player.physics()
end
function sdk.rig.parts() return sdk.rig.read({"parts"}).parts or {} end
function sdk.rig.joints() return sdk.rig.read({"joints"}).joints or {} end
function sdk.rig.contacts() return sdk.rig.read({"contacts"}).contacts or {} end
function sdk.rig.configure_joint(index,options) sdk.player.set_joint(index,options) end
function sdk.rig.reset_joint(index) sdk.player.reset_joint(index) end
function sdk.rig.reset() sdk.player.reset_joints() end
sdk.bodies = {}
function sdk.bodies.read(ref)
    if ref.kind=="mod" then return sdk.physics.read(ref.key) end
    local rig=sdk.rig.read();local list=ref.kind=="skater" and rig.parts or ref.kind=="board" and rig.board
    for _,b in ipairs(list or {}) do if b.index==ref.index then return b end end
end
function sdk.bodies.impulse(ref,value,point)
    if ref.kind=="mod" then sdk.physics.impulse(ref.key,value,point)
    else submit{kind="native_impulse",body={kind=ref.kind,index=ref.index},impulse=value,point=point,angular=false} end
end
function sdk.bodies.angular_impulse(ref,value)
    if ref.kind=="mod" then sdk.physics.torque_impulse(ref.key,value)
    else submit{kind="native_impulse",body={kind=ref.kind,index=ref.index},impulse=value,angular=true} end
end
function sdk.input.override_action(id,value)
    if type(id)=='string' then id=action_id(id) end
    submit{kind="input_override",action=id,value=value}
end

sdk.graphs = {}
function sdk.graphs.read(graph) return (sdk.engine.read("graphs") or {})[graph] end
function sdk.graphs.set_enabled(graph,target,index,enabled) submit{kind="graph_gate",graph=graph,target=target,index=index,enabled=enabled} end

function sdk.rig.configure_part(index,options) submit{kind="rig_part",index=index,options=options} end
function sdk.rig.reset_part(index) sdk.rig.configure_part(index,nil) end

-- Retail menus (capability retail_menus = 1): the pause menu and Game Settings structure.
-- Entries: kind 'category' | 'item' | 'setting', keyed by retail internal names; new ones
-- are named '<mod id>.<name>'. Events arrive in on_event (name menu_select / menu_highlight /
-- menu_value). Everything is removed when the mod stops.
sdk.menus = { version = 1 }
local function menu_info() return as_table(sdk.snapshot.menus) or {} end
function sdk.menus.category(id, options) submit{kind="menu_category",id=id,options=options} end
function sdk.menus.item(id, options) submit{kind="menu_item",id=id,options=options} end
function sdk.menus.setting(id, options) submit{kind="menu_setting",id=id,options=options} end
function sdk.menus.hide(entry, id, modes) submit{kind="menu_hide",entry=entry,id=id,modes=modes} end
function sdk.menus.handle(entry, id, options) submit{kind="menu_handle",entry=entry,id=id,options=options} end
function sdk.menus.unhandle(entry, id) submit{kind="menu_unhandle",entry=entry,id=id} end
-- set_value: the mod's own top layer takes it as a menu change; on any other row (engine settings,
-- other mods) it is held in this mod's own layer on top, applied but never saved, removed on stop.
function sdk.menus.set_value(id, value) submit{kind="menu_set_value",id=id,value=value} end
function sdk.menus.value(id) return (as_table(menu_info().values) or {})[id] end
function sdk.menus.info() return menu_info() end
-- Supply or replace a front-end APT movie by its retail import name (e.g. 'source/controls/panel')
-- with a JSON file in the mod's folder (the format setup exports to assets/private/menu-movies).
-- Every menu importing it is relinked; the newest mod wins; removed when the mod stops.
function sdk.menus.movie(name, path) submit{kind="menu_movie",name=name,path=path} end
-- Replace one bitmap of a menu movie by its stable id: the movie's import name plus the bitmap
-- character id (sdk.menus.info().movies.bitmaps lists them), with a PNG in the mod's folder. The
-- image covers the same fill area as the retail bitmap; newest mod wins; removed when the mod stops.
function sdk.menus.bitmap(movie, bitmap, path) submit{kind="menu_bitmap",movie=movie,bitmap=bitmap,path=path} end
