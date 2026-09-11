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
function sdk.ui.text(key, text) submit{kind="overlay",key=key,text=text} end
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

sdk.graphics = { version = 2 }
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
        key=key,
        path=opts.path or "",
        body=opts.body,
        position=opts.position,
        rotation=opts.rotation,
        scale=opts.scale or {1,1,1},
        color=opts.color or {0.85,0.85,0.9},
        visible=opts.visible ~= false,
    }
end
function sdk.graphics.remove(key) submit{kind="graphics_remove",key=key} end
function sdk.graphics.set_visible(key, visible) submit{kind="graphics_visibility",key=key,visible=visible and true or false} end

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

sdk.player = {}
function sdk.player.read() return as_table(sdk.snapshot.player) or {} end
function sdk.player.attach(body, offset) submit{kind="player_attach",body=body,offset=offset or {0,0,0}} end
function sdk.player.detach(options) submit{kind="player_detach",options=options or {}} end
function sdk.player.detach_error() return sdk.snapshot.detach_error end
function sdk.player.detaching() return sdk.snapshot.detach_pending == true end
function sdk.player.attached()
    local a = as_table(sdk.snapshot.attach)
    return a and a.body or nil
end

sdk.camera = { version = 1 }
-- Persistent, render-rate camera. Does not write the body's pose or velocity.
function sdk.camera.rig(body, options)
    submit{kind="camera_rig",body=body,options=options or {}}
end
function sdk.camera.clear() sdk.camera.clear_follow() end
function sdk.camera.follow(body, offset) submit{kind="camera_follow",body=body,offset=offset or {0,2.5,-6}} end
function sdk.camera.clear_follow() submit{kind="camera_follow",body=nil,offset={0,2.5,-6}} end
function sdk.camera.set(position, look_at) submit{kind="camera_set",position=position,look_at=look_at} end

sdk.input = {}
function sdk.input.down(key) return sdk.snapshot.keys[key] == true end
function sdk.input.action(id)
    assert(type(id)=='number' and id%1==0 and id>=64 and id<=81,'action ID must be 64..81')
    return sdk.snapshot.actions[id-63]
end
function sdk.input.pad()
    return as_table(sdk.snapshot.pad) or {buttons=0,triggers={0,0},left={0,0},right={0,0}}
end

sdk.assets = {}
function sdk.assets.objects(path)
    return assets_objects(path)
end

sdk.net = {}
function sdk.net.info() return as_table(sdk.snapshot.network) or {active=false,local_id="0",is_host=true,states={},status=""} end
function sdk.net.publish(key,value) submit{kind="network_state",key=key,value=value} end
function sdk.net.read(peer,key)
  local n = as_table(sdk.snapshot.network) or {}
  return ((((n.states or {})[sdk.mod_id] or {})[tostring(peer)]) or {})[key]
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
function sdk._advance(dt)
    sdk.time.elapsed=sdk.time.elapsed+dt
    local due={}
    for key,timer in pairs(timers) do if timer.at<=sdk.time.elapsed then due[#due+1]=key end end
    table.sort(due)
    for _,key in ipairs(due) do
        local timer=timers[key]
        if timer and timer.at<=sdk.time.elapsed then timers[key]=nil; timer.callback() end
    end
end
