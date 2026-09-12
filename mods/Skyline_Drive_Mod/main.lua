-- Model collision repair 4.3.1: collision cooked from the SAME render GLB; no sidecar assets.
-- Driving edition 1: latest working vehicle + unchanged audio; render-rate cameras,
-- retained dashboard, progressive driver-input mapping, explicit optional aero.
-- Skyline Physics v4 -- Lua 5.4 / supplied Skate SDK API 2.
-- Reduced-order vehicle: one 6-DOF chassis, four rotating tire DOFs,
-- passive clutch/differential/brakes, and a combined-slip brush tire law.
-- No yaw targets, countersteer assist, slide-recovery state, lateral-velocity
-- deletion, handbrake grip multiplier, or upright torque. Aero is an explicit load.
-- See README.md for the collision/API contract and validation limits.

local BODY, MODEL = "chassis", "skyline.glb"
local C = {
    mass = 1400, gravity = 9.81,
    -- At nominal ride height: CG is 0.400 m above a level road.
    -- This is an explicitly tuned chassis, NOT measured factory Skyline data.
    center = {0, -0.27160, 0.16},
    inertia_half = {0.92, 0.55, 2.10},
    -- Physics uses the existing rendered body node, cooked into multiple
    -- convex solids by the host. No separate collision GLB/JSON or point list.
    entry_radius = 1.20, reentry_delay = 5.0,
    radius = 0.341,
    ride_length = 0.20, droop_length = 0.37,
    front_spring = 37000, rear_spring = 32000,
    damping_ratio = 0.85,
    front_antiroll = 8000, rear_antiroll = 6800,
    antiroll_damping = 450,
    bump_length = 0.055, bump_stiffness = 110000,
    maximum_load = 22000,
    wheel_inertia = 1.85,
    -- Cornering/longitudinal stiffness in N per unit normalized slip, not N/(m/s).
    front_stiffness_per_load = 23.0, rear_stiffness_per_load = 24.0,
    load_exponent = 0.90,
    front_drive = 0.15,
    brake_torque_total = 6800, front_brake_fraction = 0.72,
    handbrake_torque = 3800, rolling_coefficient = 0.012,
    rear_diff_preload = 12, rear_diff_ramp = 0.18,
    steer_limit = math.rad(40), steer_rate = math.rad(170),
    steering_exponent = 1.70,
    final_drive = 3.545, reverse_ratio = 3.280,
    gear_ratios = {3.827, 2.360, 1.685, 1.312, 1.000, 0.793},
    idle_rpm = 900, redline_rpm = 8000, shift_time = 0.18,
    engine_inertia = 0.30, clutch_capacity = 620,
    iterations = 8, tire_root_iterations = 10,
    aero_drag = 0.42,
}
-- Model's named left wheels are +X; model forward is +Z.
-- A right input therefore requests NEGATIVE yaw, matching a rear chase view.
local wheels = {
    {name="FL", key="wheel_fl", pos={ 0.794235,-0.330559, 1.443641}, front=true, mate=2},
    {name="FR", key="wheel_fr", pos={-0.796149,-0.330652, 1.443556}, front=true, mate=1},
    {name="RL", key="wheel_rl", pos={ 0.795353,-0.330587,-1.347033}, front=false,mate=4},
    {name="RR", key="wheel_rr", pos={-0.796153,-0.330635,-1.347024}, front=false,mate=3},
}
local wheelbase = 1.44360 + 1.34703
local track = 1.591
local I = {
    C.mass/3 * (C.inertia_half[2]^2 + C.inertia_half[3]^2),
    C.mass/3 * (C.inertia_half[1]^2 + C.inertia_half[3]^2),
    C.mass/3 * (C.inertia_half[1]^2 + C.inertia_half[2]^2),
}
local state = {
    spawned=false, occupied=false, pending=nil, gear=1,
    rpm=C.idle_rpm, engine_omega=C.idle_rpm*math.pi/30, clutch=0,
    steer=0, shift=0, time=0, enter_after=0,
    keys={}, buttons=0, hud_time=0, mass_audit=nil, keyboard_axis=0, aero_load=0,
    enter_requested=false, exit_requested=false, reenter_on_ready=false,
    hull_debug=false, interaction_text="", mp_hint_time=0,
}

local function clamp(x,a,b) return math.max(a,math.min(b,x)) end
local function finite(x) return type(x)=="number" and x==x and math.abs(x)<math.huge end
local function number(x, fallback) return finite(x) and x or fallback end
local function vec(x) return type(x)=="table" and finite(x[1]) and finite(x[2]) and finite(x[3]) end
local function add(a,b) return {a[1]+b[1],a[2]+b[2],a[3]+b[3]} end
local function sub(a,b) return {a[1]-b[1],a[2]-b[2],a[3]-b[3]} end
local function mul(a,s) return {a[1]*s,a[2]*s,a[3]*s} end
local function dot(a,b)
    if not vec(a) or not vec(b) then return 0 end
    return a[1]*b[1]+a[2]*b[2]+a[3]*b[3]
end
local function cross(a,b) return {a[2]*b[3]-a[3]*b[2],a[3]*b[1]-a[1]*b[3],a[1]*b[2]-a[2]*b[1]} end
local function norm(a)
    if not vec(a) then return nil end
    local l=math.sqrt(dot(a,a))
    return l>1e-8 and mul(a,1/l) or nil
end
local function rotate(q,v)
    local t=mul(cross({q[1],q[2],q[3]},v),2)
    return add(v,add(mul(t,q[4]),cross({q[1],q[2],q[3]},t)))
end
local function unrotate(q,v) return rotate({-q[1],-q[2],-q[3],q[4]},v) end
local function inertia(q,v,inverse)
    local a=unrotate(q,v)
    for i=1,3 do a[i]=inverse and a[i]/I[i] or a[i]*I[i] end
    return rotate(q,a)
end
local function yaw(q) return math.atan(2*(q[4]*q[2]+q[1]*q[3]),1-2*(q[2]^2+q[3]^2)) end
local function yaw_rotation(a) return {0,math.sin(a/2),0,math.cos(a/2)} end
local function approach(x,target,step) return x+clamp(target-x,-step,step) end
local function deadzone(x,threshold)
    if math.abs(x)<=threshold then return 0 end
    return (x<0 and -1 or 1)*(math.abs(x)-threshold)/(1-threshold)
end
local function gear_ratio()
    if state.gear==0 then return 0 end
    return (state.gear<0 and -C.reverse_ratio or C.gear_ratios[state.gear])*C.final_drive
end
local function gear_label() return state.gear<0 and "R" or (state.gear==0 and "N" or tostring(state.gear)) end
local function down(key) return sdk.input.down(key)==true end
local function edge(key)
    local held=down(key)
    local yes=held and not state.keys[key]
    state.keys[key]=held
    return yes
end
local function pressed_button(buttons,mask)
    return (buttons & mask)~=0 and (state.buttons & mask)==0
end
-- AUDIO EXTENSION: single engine sample, pitch-shifted by RPM.
local sound = {
    running=false, supported=false, prepared=false, clock=0, cooldown=0,
    rpm=900, throttle=0, shift=0, previous_throttle=0,
    overrun_remaining=0, pop_slot=0, random_state=73641,
    gain=0, shift_gain=1, load=0, log_pitch=nil,
    turbo_gain=0, turbo_pitch=1, brake=0, brake_volume=0,
    drift_slip=0, drift_slip_smooth=0, drift_volume=0, brake_pitch=1,
}
local DRIFT_SLIP_MIN = 0.28
local ENGINE_PATH = "audio/engine.wav"
local ENGINE_BASE_RPM = 3400
local BRAKE_PATH = "audio/brake.wav"
local function smooth_toward(current,target,dt,tau)
    return current+(target-current)*(1-math.exp(-dt/math.max(tau,1e-4)))
end
local function smooth_rpm_toward(current,target,dt)
    local delta=target-current
    local tau=0.20+0.55*clamp(math.abs(delta)/2800,0,1)
    return smooth_toward(current,target,dt,tau)
end
local function sound_random()
    sound.random_state=(sound.random_state*48271)%2147483647
    return sound.random_state/2147483647
end
local function stop_audio()
    if sound.supported then sdk.audio.stop_all() end
    sound.running=false; sound.cooldown=0; sound.overrun_remaining=0
    sound.previous_throttle=0
    sound.gain=0; sound.shift_gain=1; sound.load=0; sound.log_pitch=nil
    sound.turbo_gain=0; sound.turbo_pitch=1
    sound.brake=0; sound.brake_volume=0
    sound.drift_slip=0; sound.drift_slip_smooth=0
    sound.drift_volume=0; sound.brake_pitch=1
end
local function prepare_audio()
    sound.supported=type(sdk.audio)=="table" and number(sdk.audio.version,0)>=1
    if not sound.supported then
        sdk.log("Skyline audio unavailable: install the native Audio API update and rebuild the game.")
        sdk.ui.text("skyline_audio","Sound requires native Audio API extension 1. Physics still works.")
        return
    end
    if sound.prepared then return end
    sdk.audio.preload(ENGINE_PATH)
    sdk.audio.preload("audio/turbo_loop.wav")
    sdk.audio.preload("audio/lift.wav")
    sdk.audio.preload("audio/pop.wav")
    sdk.audio.preload(BRAKE_PATH)
    sound.prepared=true
    sdk.ui.text("skyline_audio","")
end
local function start_audio()
    if not sound.supported or sdk.settings.audio_enabled==false or sound.running then return end
    sound.clock=0; sound.cooldown=0
    sound.rpm=math.max(state.rpm,200)
    sound.throttle=0; sound.previous_throttle=0; sound.overrun_remaining=0
    sound.gain=0; sound.shift_gain=1; sound.load=0
    sound.log_pitch=math.log(math.max(sound.rpm,200)/ENGINE_BASE_RPM)
    sound.turbo_gain=0; sound.turbo_pitch=1
    local pitch=clamp(math.exp(sound.log_pitch),0.25,3)
    local pitch_b=clamp(pitch*1.005,0.25,3)
    sdk.audio.play("engine_main",{
        path=ENGINE_PATH,body=BODY,offset={0,-0.1,0.45},loop=true,volume=0,
        pitch=pitch,spatial=true,spatial_scale=0.10,fade_in=0.08,
    })
    sdk.audio.play("engine_main_b",{
        path=ENGINE_PATH,body=BODY,offset={0,-0.1,0.45},loop=true,volume=0,
        pitch=pitch_b,spatial=true,spatial_scale=0.10,fade_in=0.08,
    })
    sdk.audio.play("turbo",{
        path="audio/turbo_loop.wav",body=BODY,offset={0,0,0.8},
        loop=true,volume=0,pitch=1,spatial=true,spatial_scale=0.1,fade_in=0.08,
    })
    sdk.audio.play("brake",{
        path=BRAKE_PATH,body=BODY,offset={0,-0.25,-0.9},
        loop=true,volume=0,pitch=1,spatial=true,spatial_scale=0.12,fade_in=0.10,
    })
    sdk.audio.play("brake_b",{
        path=BRAKE_PATH,body=BODY,offset={0,-0.25,-0.9},
        loop=true,volume=0,pitch=1.028,spatial=true,spatial_scale=0.12,fade_in=0.10,
    })
    sound.running=true
end
local function exhaust_pop(intensity)
    if not sound.running then return end
    local master=clamp(number(sdk.settings.engine_volume,0.70),0,1)
    local volume=clamp(number(sdk.settings.pop_volume,0.65),0,1)
    if master*volume<=0 then return end
    sound.pop_slot=sound.pop_slot%6+1
    sdk.audio.play("exhaust_pop_"..sound.pop_slot,{
        path="audio/pop.wav",
        body=BODY,offset={-0.5,-0.22,-1.95},loop=false,
        volume=clamp(master*volume*intensity,0,1),
        pitch=0.90+0.22*sound_random(),spatial=true,spatial_scale=0.1,fade_in=0,
    })
end
local function update_audio(event)
    if not sound.supported then return end
    if not state.spawned or sdk.settings.audio_enabled==false then
        if sound.running then stop_audio() end
        return
    end
    if not sound.running then start_audio() end
    if sdk.snapshot.paused or sdk.snapshot.replay then return end
    local dt=clamp(number(event and event.dt,1/60),0,1/30)
    sound.clock=sound.clock+dt
    local actual_rpm=clamp(number(state.rpm,0),0,12000)
    sound.rpm=smooth_rpm_toward(sound.rpm,actual_rpm,dt)
    local throttle=clamp(number(sound.throttle,0),0,1)
    local master=clamp(number(sdk.settings.engine_volume,0.70),0,1)
    sound.cooldown=math.max(0,sound.cooldown-dt)
    local limiter=actual_rpm>=C.redline_rpm-160 and throttle>0.55 and sound.shift<=0
    if limiter and sdk.settings.redline_pops~=false then
        sound.overrun_remaining=0
        if sound.cooldown<=0 then
            exhaust_pop(0.78+0.20*sound_random())
            sound.cooldown=0.095+0.075*sound_random()
        end
    else
        local lifted=sound.previous_throttle>0.6 and throttle<0.2 and actual_rpm>4000
        if lifted then
            if sdk.settings.overrun_pops~=false then
                sound.overrun_remaining=2+math.floor(sound_random()*2)
                sound.cooldown=0.04+0.06*sound_random()
            end
            if sdk.settings.turbo_audio~=false and master>0 then
                sdk.audio.play("turbo_lift",{
                    path="audio/lift.wav",body=BODY,offset={0,0,0.8},
                    volume=master*0.55,pitch=0.85+0.2*sound_random(),
                    spatial=true,spatial_scale=0.1,fade_in=0,
                })
            end
        end
        if throttle>0.3 or actual_rpm<3000 or sdk.settings.overrun_pops==false then
            sound.overrun_remaining=0
        end
        if sound.overrun_remaining>0 and sound.cooldown<=0 then
            exhaust_pop(0.36+0.24*sound_random())
            sound.overrun_remaining=sound.overrun_remaining-1
            sound.cooldown=0.11+0.12*sound_random()
        end
    end
    sound.previous_throttle=throttle
    local brake_in=clamp(number(sound.brake,0),0,1)
    local speed=clamp(number(state.speed,0),0,200)
    local speed_factor=clamp((speed-0.5)/12,0,1)
    local brake_master=clamp(number(sdk.settings.brake_volume,0.55),0,1)
    sound.drift_slip_smooth=smooth_toward(sound.drift_slip_smooth,number(sound.drift_slip,0),dt,0.14)
    local slip=sound.drift_slip_smooth
    local drift_mix=0
    if slip>DRIFT_SLIP_MIN then
        local norm=clamp((slip-DRIFT_SLIP_MIN)/2.6,0,1)
        drift_mix=norm*norm*norm
    end
    local pedal_brake=brake_in*speed_factor
    local drift_target=drift_mix*speed_factor*0.68
    local drift_tau=drift_target>sound.drift_volume and 0.22 or 0.14
    sound.drift_volume=smooth_toward(sound.drift_volume,drift_target,dt,drift_tau)
    local brake_mix=math.max(pedal_brake,sound.drift_volume)
    local brake_target=(sdk.settings.brake_audio==false) and 0 or brake_master*brake_mix
    sound.brake_volume=smooth_toward(sound.brake_volume,brake_target,dt,0.16)
    local norm_pitch=clamp((slip-DRIFT_SLIP_MIN)/3.5,0,1)
    local wobble=0.012*math.sin(sound.clock*4.3)+0.008*math.sin(sound.clock*7.1)
    sound.brake_pitch=smooth_toward(sound.brake_pitch,0.98+0.08*norm_pitch+wobble,dt,0.22)
    local brake_vol=sound.brake_volume
    sdk.audio.update("brake",{
        volume=brake_vol*0.72,
        pitch=clamp(sound.brake_pitch,0.25,4),
    })
    sdk.audio.update("brake_b",{
        volume=brake_vol*0.28,
        pitch=clamp(sound.brake_pitch*1.028,0.25,4),
    })
    local rpm=math.max(sound.rpm,200)
    sound.load=smooth_toward(sound.load,throttle^0.70,dt,0.22)
    local load=sound.load
    local alive=clamp((rpm-150)/600,0,1)
    sound.shift_gain=smooth_toward(sound.shift_gain,sound.shift>0 and 0.82 or 1.0,dt,0.18)
    local drift_gain=1.0+0.07*drift_mix
    local target=master*0.50*alive*(0.70+0.30*load)*sound.shift_gain*drift_gain
    sound.gain=smooth_toward(sound.gain,target,dt,0.20)
    local gain=sound.gain
    local target_log=math.log(rpm/ENGINE_BASE_RPM)
    sound.log_pitch=smooth_toward(sound.log_pitch or target_log,target_log,dt,0.26)
    local pitch=clamp(math.exp(sound.log_pitch),0.25,3)
    local pitch_b=clamp(pitch*1.005,0.25,3)
    sdk.audio.update("engine_main",{
        volume=gain*0.70,pitch=pitch,
    })
    sdk.audio.update("engine_main_b",{
        volume=gain*0.30,pitch=pitch_b,
    })
    local boost_target=clamp((rpm-2000)/4500,0,1)*load
    sound.turbo_gain=smooth_toward(sound.turbo_gain,boost_target,dt,0.24)
    sound.turbo_pitch=smooth_toward(sound.turbo_pitch,clamp(0.65+rpm/9000,0.25,4),dt,0.22)
    sdk.audio.update("turbo",{
        volume=sdk.settings.turbo_audio==false and 0 or master*0.16*sound.turbo_gain,
        pitch=sound.turbo_pitch,
    })
end
-- END AUDIO EXTENSION

-- DRIVING PRESENTATION: no writes to physical body state.
local presentation = {
    mode="chase", supported=false, debug=false, hud_timer=0,
    speed=0, handbrake=false, active_hud=false,
}
local function clear_debug_text()
    for _,key in ipairs({"skyline_help","skyline_status","skyline_handling","skyline_wheels"}) do
        sdk.ui.text(key,"")
    end
end
local function set_debug_visible(visible)
    presentation.debug=visible
    clear_debug_text()
    if visible then
        sdk.ui.text("skyline_help","F10 spawn | E/Y enter | RT/W gas | LT/S brake | B/Shift handbrake | RB/X up, LB/Z down | N neutral | Xbox X/C camera | H debug | R/R3 reset")
    end
end
local function configure_camera()
    if not state.occupied then return end
    presentation.hud_timer=0
    if not presentation.supported then
        sdk.camera.follow(BODY,{0,2.1,-5.8})
        return
    end
    local hood=presentation.mode=="hood"
    sdk.camera.rig(BODY,{
        mode=presentation.mode,
        distance=clamp(number(sdk.settings.camera_distance,5.8),3.5,10),
        distance_gain=1.8,height=1.9,height_gain=0.25,target_height=0.78,
        look_ahead=0.12,velocity_heading=0.18,
        spring_hz=clamp(number(sdk.settings.camera_spring,2.8),1,6),
        heading_half_life=0.14,acceleration_lag=0.018,speed_reference=55.556,
        fov=hood and 60 or clamp(number(sdk.settings.camera_fov,55),40,80),
        fov_gain=hood and 0 or 8,near=hood and 0.04 or 0.07,
        collision=true,collision_radius=0.28,
        hood_offset={0,clamp(number(sdk.settings.hood_height,0.48),0.30,0.9),1.05},
    })
end
local function clear_presentation()
    sdk.camera.clear_follow()
    if presentation.supported then sdk.ui.remove("driving_dashboard") end
    presentation.active_hud=false;presentation.hud_timer=0
    clear_debug_text()
end
-- GTA-style VFX: textured camera billboards (smoke) + persistent rubber decals (skids).
-- Transparent sort is ascending distance: lower bias draws behind, higher draws in front.
local SKID_BUFFER_OPTS={blend=true,unlit=true,texture="textures/skid_tread.png",depth_bias=-3.5,tint={0.05,0.05,0.055}}
local SMOKE_BUFFER_OPTS={blend=true,unlit=true,texture="textures/smoke_soft.png",depth_bias=4.5,tint={0.82,0.82,0.85}}
local SKID_CHUNK_MAX_VERTS=8192
local SKID_POINTS_PER_UPLOAD=96
local SKID_PTS_TAIL=40
-- Host allows 32 mesh buffers/mod; reserve smoke + remote VFX headroom.
local MAX_LOCAL_SKID_BUFFERS=20
local MAX_REMOTE_SKID_BUFFERS=8
local MAX_REMOTE_VFX_PEERS=4
local SKID_ALPHA_MIN=0.10
local SKID_ALPHA_MAX=0.72
local MAX_SMOKE_SPAWN_PER_TICK=2
local vfx={
    supported=false,skid_kappa=0.10,skid_alpha=5.0,
    skid_width=0.32,min_skid_dist=0.03,skid_lift=0.012,
    smoke_lifetime=2.0,smoke_size={0.47,1.88},smoke_slip_min=0.30,
    smoke_mesh_live=false,smoke_dirty=false,
    remote_wheels={},remote_smoke={},remote_smoke_dirty=false,
    remote_smoke_mesh_live=false,
}
local REMOTE_WHEEL_KEYS={rr="wheel_rr",rl="wheel_rl"}
local function vfx_net_active()
    return sdk.net and type(sdk.net.info)=="function" and (sdk.net.info().active==true)
end
local function refresh_mp_status(dt)
    if not sdk.net or type(sdk.net.info)~="function" then
        sdk.ui.text("skyline_mp","")
        return
    end
    local info=sdk.net.info()
    if not info or info.active~=true then
        sdk.ui.text("skyline_mp","")
        return
    end
    state.mp_hint_time=math.max(0,(state.mp_hint_time or 0)-dt)
    if state.mp_hint_time>0 then return end
    state.mp_hint_time=0.5
    local lines={
        state.spawned
            and "Your car is spawned and should replicate to other players."
            or "Press F10 to spawn your car. Other players cannot see your vehicle until you do.",
        "Both players need Skyline enabled with identical mod files and the skyline-driving-update build.",
    }
    if vfx.supported then
        lines[#lines+1]=vfx.mesh
            and "VFX buffers ready (remote smoke/skids work without spawning your own car)."
            or "VFX supported but buffers not ready yet."
    else
        lines[#lines+1]="VFX unsupported: update to a build with mesh_buffer (graphics API 4)."
    end
    if info.status and info.status~="" then lines[#lines+1]=info.status end
    sdk.ui.text("skyline_mp",table.concat(lines,"\n"))
end
local function clear_mesh(mesh)
    mesh.positions={};mesh.normals={};mesh.colors={};mesh.uvs={};mesh.indices={}
end
local function camera_position()
    local snap=sdk.snapshot
    return snap and snap.camera and vec(snap.camera.position) and snap.camera.position
end
local function billboard_axes(center,cam)
    local to=sub(cam,center)
    local dist=math.sqrt(dot(to,to))
    if dist<1e-4 then return {1,0,0},{0,1,0},{0,0,-1} end
    to=mul(to,1/dist)
    local right=cross({0,1,0},to)
    local rlen=math.sqrt(dot(right,right))
    if rlen<1e-4 then right={1,0,0} else right=mul(right,1/rlen) end
    local up=cross(to,right)
    return right,up,to
end
local function push_rgba(mesh,a)
    mesh.colors[#mesh.colors+1]={1,1,1,a}
end
local function append_smoke_sprite(mesh,center,right,up,fwd,size,alpha)
    local hs=size*0.5
    local r=mul(right,hs); local u=mul(up,hs)
    local base=#mesh.positions
    local n={-fwd[1],-fwd[2],-fwd[3]}
    mesh.positions[#mesh.positions+1]=sub(sub(center,r),u)
    mesh.positions[#mesh.positions+1]=add(sub(center,r),u)
    mesh.positions[#mesh.positions+1]=add(add(center,r),u)
    mesh.positions[#mesh.positions+1]=sub(add(center,r),u)
    mesh.uvs[#mesh.uvs+1]={0,1}; mesh.uvs[#mesh.uvs+1]={1,1}
    mesh.uvs[#mesh.uvs+1]={1,0}; mesh.uvs[#mesh.uvs+1]={0,0}
    for _=1,4 do mesh.normals[#mesh.normals+1]=n; push_rgba(mesh,alpha) end
    mesh.indices[#mesh.indices+1]=base; mesh.indices[#mesh.indices+1]=base+1; mesh.indices[#mesh.indices+1]=base+2
    mesh.indices[#mesh.indices+1]=base; mesh.indices[#mesh.indices+1]=base+2; mesh.indices[#mesh.indices+1]=base+3
end
local function clear_session_skids()
    for _,w in ipairs(wheels) do
        if w.skid_chunks then
            for _,chunk in ipairs(w.skid_chunks) do sdk.graphics.remove(chunk.key) end
        end
        w.skid_chunks=nil; w.skid_pts=nil; w.skid_meshed=nil; w.skid_u=nil
        w.skid_active=nil
        w.smoke_accum=nil
    end
    vfx.skid_dirty=false
end
local function clear_smoke()
    if not vfx.supported then return end
    sdk.graphics.remove("vfx_smoke")
    vfx.smoke=nil
    if vfx.mesh then vfx.mesh.smoke={positions={},normals={},colors={},uvs={},indices={}} end
end
local function clear_remote_vfx(peer)
    if not vfx.supported then return end
    local wheels_list=vfx.remote_wheels and vfx.remote_wheels[peer]
    if wheels_list then
        for _,w in ipairs(wheels_list) do
            if w.skid_chunks then
                for _,chunk in ipairs(w.skid_chunks) do sdk.graphics.remove(chunk.key) end
            end
        end
        vfx.remote_wheels[peer]=nil
    end
    if vfx.remote_smoke then vfx.remote_smoke[peer]=nil end
    vfx.remote_smoke_dirty=true
end
local function clear_all_remote_vfx()
    if not vfx.remote_wheels then return end
    for peer in pairs(vfx.remote_wheels) do clear_remote_vfx(peer) end
    vfx.remote_wheels={}
    vfx.remote_smoke={}
    vfx.remote_smoke_dirty=false
    if vfx.supported then
        sdk.graphics.remove("vfx_smoke_remote")
        vfx.remote_smoke_mesh_live=false
        if vfx.mesh then vfx.mesh.remote_smoke={positions={},normals={},colors={},uvs={},indices={}} end
    end
end
local function clear_effects()
    clear_smoke(); clear_all_remote_vfx()
end
local function flatten_forward(fwd,normal)
    if not vec(fwd) or not vec(normal) then return fwd end
    local flat=sub(fwd,mul(normal,dot(fwd,normal)))
    return norm(flat) or fwd
end
local function segment_forward(pt_a,pt_b)
    if not pt_a or not pt_b or not vec(pt_a.pos) or not vec(pt_b.pos) then return nil end
    return flatten_forward(sub(pt_b.pos,pt_a.pos),pt_b.normal or pt_a.normal)
end
local function new_skid_delta()
    return {positions={},uvs={},indices={}}
end
local SKID_TEX_REPEAT=1.8
local function push_skid_vertex_color(delta,alpha)
    delta.colors=delta.colors or {}
    local a=clamp(number(alpha,SKID_ALPHA_MIN),SKID_ALPHA_MIN,SKID_ALPHA_MAX)
    delta.colors[#delta.colors+1]=1
    delta.colors[#delta.colors+1]=1
    delta.colors[#delta.colors+1]=1
    delta.colors[#delta.colors+1]=a
end
local function sane_point(p)
    if not vec(p) then return false end
    for i=1,3 do
        local v=p[i]
        if v~=v or v<-100000 or v>100000 then return false end
    end
    return true
end
local function skid_uv_u(u)
    -- Host rejects UVs outside [-1000, 1000]; wrap so long drifts stay valid.
    local v=number(u,0)%64.0
    if v<-1000 then v=-1000 elseif v>1000 then v=1000 end
    return v
end
local function push_cross_section(delta,pt,forward,width)
    local lateral=norm(cross(forward,pt.normal)); if not lateral then return false end
    if not sane_point(pt.pos) then return false end
    local half=width*0.5
    local u=skid_uv_u(pt.u)
    local a=pt.intensity or SKID_ALPHA_MIN
    delta.positions[#delta.positions+1]=add(pt.pos,mul(lateral,half))
    push_skid_vertex_color(delta,a)
    delta.uvs[#delta.uvs+1]=u; delta.uvs[#delta.uvs+1]=0
    delta.positions[#delta.positions+1]=sub(pt.pos,mul(lateral,half))
    push_skid_vertex_color(delta,a)
    delta.uvs[#delta.uvs+1]=u; delta.uvs[#delta.uvs+1]=1
    return true
end
local function active_skid_chunk(w)
    local chunks=w.skid_chunks
    if not chunks then return nil end
    local chunk=chunks[#chunks]
    if chunk and not chunk.sealed then return chunk end
    return nil
end
local function reset_skid_pending(w)
    local chunk=active_skid_chunk(w)
    if chunk then chunk.pending=new_skid_delta() end
end
local function skid_delta_valid(chunk,delta)
    if not delta or #delta.positions==0 then return true end
    if #delta.indices<3 then return false end
    if delta.colors and #delta.colors~=#delta.positions*4 then return false end
    if delta.uvs and #delta.uvs~=#delta.positions*2 then return false end
    local total=chunk.verts+#delta.positions
    for _,idx in ipairs(delta.indices) do
        if idx<chunk.verts or idx>=total then return false end
    end
    return total<=SKID_CHUNK_MAX_VERTS
end
local function flush_skid_delta(chunk)
    local delta=chunk.pending
    if not delta or #delta.positions==0 then return true end
    if #delta.indices<3 then
        chunk.pending=new_skid_delta()
        return true
    end
    if not skid_delta_valid(chunk,delta) then
        chunk.pending=new_skid_delta()
        return false
    end
    if not chunk.live then
        sdk.graphics.mesh_buffer(chunk.key,SKID_BUFFER_OPTS)
        chunk.live=true
        vfx.smoke_dirty=true
    end
    sdk.graphics.mesh_buffer_append(chunk.key,delta)
    chunk.verts=chunk.verts+#delta.positions
    chunk.pending=new_skid_delta()
    return true
end
local function iter_skid_wheels(remote)
    local list={}
    if remote then
        for _,wheels_list in pairs(vfx.remote_wheels or {}) do
            for _,w in ipairs(wheels_list) do list[#list+1]=w end
        end
    else
        for _,w in ipairs(wheels) do list[#list+1]=w end
    end
    return list
end
local function count_skid_buffers(remote)
    local n=0
    for _,w in ipairs(iter_skid_wheels(remote)) do
        if w.skid_chunks then
            for _,chunk in ipairs(w.skid_chunks) do
                if chunk.live then n=n+1 end
            end
        end
    end
    return n
end
local function evict_oldest_sealed_skid(remote)
    local victim_wheel,victim_idx=nil,nil
    local oldest=math.huge
    for _,w in ipairs(iter_skid_wheels(remote)) do
        local chunks=w.skid_chunks
        if chunks then
            for i,chunk in ipairs(chunks) do
                if chunk.sealed and (chunk.serial or 0)<oldest then
                    oldest=chunk.serial or 0
                    victim_wheel=w
                    victim_idx=i
                end
            end
        end
    end
    if not victim_wheel then return false end
    local chunk=victim_wheel.skid_chunks[victim_idx]
    sdk.graphics.remove(chunk.key)
    table.remove(victim_wheel.skid_chunks,victim_idx)
    return true
end
local function recover_skid_chunk(w)
    local chunk=active_skid_chunk(w)
    if not chunk then return nil end
    reset_skid_pending(w)
    seal_skid_chunk(chunk)
    return ensure_wheel_skid_chunk(w)
end
local function append_skid_segment(chunk,pt_a,pt_b,width)
    local fwd=segment_forward(pt_a,pt_b); if not fwd then return false end
    local base=chunk.verts
    chunk.pending=new_skid_delta()
    if not push_cross_section(chunk.pending,pt_a,fwd,width) then return false end
    if not push_cross_section(chunk.pending,pt_b,fwd,width) then return false end
    chunk.pending.indices={base,base+1,base+2,base+1,base+3,base+2}
    return flush_skid_delta(chunk)
end
local function seal_skid_chunk(chunk)
    if chunk.sealed then return end
    chunk.pending=new_skid_delta()
    chunk.pending=nil
    chunk.sealed=true
end
local function end_skid_stroke(w)
    w.skid_active=false
    w.skid_pts={}
    w.skid_meshed=0
    w.skid_u=0
    reset_skid_pending(w)
end
local function ensure_wheel_skid_chunk(w)
    w.skid_chunks=w.skid_chunks or {}
    local chunk=w.skid_chunks[#w.skid_chunks]
    if chunk and not chunk.sealed then return chunk end
    local remote=w.remote_peer~=nil
    local cap=remote and MAX_REMOTE_SKID_BUFFERS or MAX_LOCAL_SKID_BUFFERS
    while count_skid_buffers(remote)>=cap do
        if not evict_oldest_sealed_skid(remote) then return nil end
    end
    w.skid_chunk_serial=(w.skid_chunk_serial or 0)+1
    local serial=w.skid_chunk_serial
    local key=serial==1 and ("vfx_skid_"..w.key) or ("vfx_skid_"..w.key.."_"..serial)
    vfx.skid_buffer_serial=(vfx.skid_buffer_serial or 0)+1
    chunk={
        key=key,verts=0,pending=new_skid_delta(),sealed=false,live=false,
        serial=vfx.skid_buffer_serial,
    }
    w.skid_chunks[#w.skid_chunks+1]=chunk
    return chunk
end
local function trim_meshed_skid_points(w)
    local pts=w.skid_pts
    if not pts then return end
    local meshed=w.skid_meshed or 0
    while meshed>2 and #pts>SKID_PTS_TAIL do
        table.remove(pts,1)
        meshed=meshed-1
    end
    w.skid_meshed=meshed
end
local function append_wheel_skids(w)
    local pts=w.skid_pts
    if not pts or #pts<2 then return end
    local meshed=w.skid_meshed or 0
    if meshed<1 then meshed=1 end
    local budget=SKID_POINTS_PER_UPLOAD
    while meshed<#pts and budget>0 do
        local chunk=ensure_wheel_skid_chunk(w)
        if not chunk then break end
        if chunk.verts+4>SKID_CHUNK_MAX_VERTS then
            reset_skid_pending(w)
            seal_skid_chunk(chunk)
            chunk=ensure_wheel_skid_chunk(w)
            if not chunk then break end
        end
        if not append_skid_segment(chunk,pts[meshed],pts[meshed+1],vfx.skid_width) then
            reset_skid_pending(w)
            chunk=recover_skid_chunk(w)
            if not chunk then break end
            if not append_skid_segment(chunk,pts[meshed],pts[meshed+1],vfx.skid_width) then break end
        end
        meshed=meshed+1
        w.skid_meshed=meshed
        budget=budget-1
    end
    trim_meshed_skid_points(w)
    if meshed<#pts then vfx.skid_dirty=true end
end
local function maybe_upload_skids()
    if not vfx.skid_dirty or not vfx.supported then return end
    for _,w in ipairs(wheels) do append_wheel_skids(w) end
    for _,wheels_list in pairs(vfx.remote_wheels or {}) do
        for _,w in ipairs(wheels_list) do append_wheel_skids(w) end
    end
    local pending=false
    local function pending_wheel(w)
        local pts=w.skid_pts
        return pts and (w.skid_meshed or 0)<#pts
    end
    for _,w in ipairs(wheels) do
        if pending_wheel(w) then pending=true; break end
    end
    if not pending then
        for _,wheels_list in pairs(vfx.remote_wheels or {}) do
            for _,w in ipairs(wheels_list) do
                if pending_wheel(w) then pending=true; break end
            end
            if pending then break end
        end
    end
    vfx.skid_dirty=pending
end
local function fill_smoke_list(mesh,smoke,cam)
    if not smoke or #smoke==0 then return false end
    local right,up,fwd=billboard_axes(smoke[#smoke].pos,cam)
    local count=0
    for i=1,#smoke do
        local p=smoke[i]
        local t=clamp(p.age/p.life,0,1)
        local fade=(1-t)^2.85
        local strength=clamp(p.intensity or 0.5,0,1)
        local birth=0.28+0.72*t
        local alpha=fade*strength*birth
        if t>=1 or alpha<=0 then goto next end
        local s=p.size*(0.60+2.05*(t^0.72))
        append_smoke_sprite(mesh,p.pos,right,up,fwd,s,clamp(alpha*0.95,0,0.68))
        count=count+1
        ::next::
    end
    return count>0
end
local function fill_smoke_mesh(mesh,cam)
    clear_mesh(mesh)
    return fill_smoke_list(mesh,vfx.smoke,cam)
end
local function fill_remote_smoke_mesh(mesh,cam)
    clear_mesh(mesh)
    local count=0
    for _,smoke in pairs(vfx.remote_smoke or {}) do
        if fill_smoke_list(mesh,smoke,cam) then count=count+1 end
    end
    return count>0
end
local function remote_smoke_intensity(slip)
    if slip<(vfx.smoke_slip_min or 0.30) then return 0 end
    local t=clamp((slip-(vfx.smoke_slip_min or 0.30))/(4.0-(vfx.smoke_slip_min or 0.30)),0,1)
    return clamp(t*t*14,0,14)
end
local function remote_skid_intensity(slip)
    if slip<0.08 then return SKID_ALPHA_MIN end
    local t=clamp(slip/4.0,0,1)
    return SKID_ALPHA_MIN+(SKID_ALPHA_MAX-SKID_ALPHA_MIN)*t*t
end
local function wheel_slip(kappa,alpha,usage)
    return math.max(kappa,alpha*0.06,math.max(0,usage-0.62)*1.8)
end
local function wheel_skid_intensity(kappa,alpha,usage)
    local slip=wheel_slip(kappa,alpha,usage)
    if slip<0.08 then return SKID_ALPHA_MIN end
    local t=clamp(slip/4.0,0,1)
    return SKID_ALPHA_MIN+(SKID_ALPHA_MAX-SKID_ALPHA_MIN)*t*t
end
local function push_skid_point(w,pos,normal,intensity)
    if not sane_point(pos) or not vec(normal) then return end
    local pts=w.skid_pts or {}
    w.skid_pts=pts
    local u=w.skid_u or 0
    local last=pts[#pts]
    intensity=clamp(number(intensity,SKID_ALPHA_MIN),SKID_ALPHA_MIN,SKID_ALPHA_MAX)
    if last and vec(last.pos) then
        local d=sub(pos,last.pos)
        local dist_sq=dot(d,d)
        local min=vfx.min_skid_dist
        if dist_sq<min*min then return end
        local dist=math.sqrt(dist_sq)
        local du=dist*SKID_TEX_REPEAT
        local i0=last.intensity or SKID_ALPHA_MIN
        if dist>min*2 then
            local steps=math.min(math.floor(dist/min)-1,6)
            for s=1,steps do
                local t=s/(steps+1)
                pts[#pts+1]={
                    pos=add(last.pos,mul(d,t)),normal=normal,u=u+du*t,
                    intensity=i0+(intensity-i0)*t,
                }
            end
        end
        u=u+du
    end
    w.skid_u=u
    pts[#pts+1]={pos=pos,normal=normal,u=u,intensity=intensity}
    vfx.skid_dirty=true
end
local function wheel_smoke_intensity(kappa,alpha,usage)
    local slip=wheel_slip(kappa,alpha,usage)
    local min_slip=vfx.smoke_slip_min or 0.30
    if slip<min_slip then return 0 end
    local t=clamp((slip-min_slip)/(4.0-min_slip),0,1)
    return clamp(t*t*14,0,14)
end
local function smoke_bucket(w)
    if w.remote_peer then
        vfx.remote_smoke=vfx.remote_smoke or {}
        local bucket=vfx.remote_smoke[w.remote_peer]
        if not bucket then
            bucket={}
            vfx.remote_smoke[w.remote_peer]=bucket
        end
        return bucket
    end
    vfx.smoke=vfx.smoke or {}
    return vfx.smoke
end
local function spawn_smoke(w,pos,intensity,dt)
    if intensity<=0 or not sane_point(pos) then return end
    w.smoke_accum=(w.smoke_accum or 0)+intensity*dt*10
    local spawned=0
    local bucket=smoke_bucket(w)
    while w.smoke_accum>=0.24 and spawned<MAX_SMOKE_SPAWN_PER_TICK do
        w.smoke_accum=w.smoke_accum-0.24
        spawned=spawned+1
        local t=clamp(intensity/14,0.12,1)
        local sz=vfx.smoke_size[1]+(vfx.smoke_size[2]-vfx.smoke_size[1])*t
        local spread=0.02*t
        bucket[#bucket+1]={
            pos=add(pos,{
                (math.random()-0.5)*spread,
                (math.random()-0.5)*spread*0.25,
                (math.random()-0.5)*spread,
            }),
            vel={0,0.48+0.62*t,0},
            age=0,life=vfx.smoke_lifetime,
            size=sz*(0.80+0.20*math.random())*(0.76+0.30*t),
            intensity=t,
        }
        if w.remote_peer then vfx.remote_smoke_dirty=true else vfx.smoke_dirty=true end
    end
end
local function tick_smoke_list(smoke,dt)
    if not smoke or #smoke==0 then return false end
    local dirty=false
    for i=#smoke,1,-1 do
        local p=smoke[i]
        p.age=p.age+dt
        if p.age>=p.life then
            table.remove(smoke,i)
            dirty=true
        else
            local lift=0.55+(p.intensity or 0.5)*0.85
            p.vel[2]=p.vel[2]+lift*dt
            p.pos=add(p.pos,mul(p.vel,dt))
            dirty=true
        end
    end
    return dirty
end
local function tick_all_smoke(dt)
    if tick_smoke_list(vfx.smoke,dt) then vfx.smoke_dirty=true end
    for _,smoke in pairs(vfx.remote_smoke or {}) do
        if tick_smoke_list(smoke,dt) then vfx.remote_smoke_dirty=true end
    end
end
local function hide_smoke_mesh()
    if not vfx.smoke_mesh_live then
        vfx.smoke_dirty=false
        return
    end
    sdk.graphics.set_visible("vfx_smoke",false)
    vfx.smoke_mesh_live=false
    vfx.smoke_dirty=false
end
local function hide_remote_smoke_mesh()
    if not vfx.remote_smoke_mesh_live then
        vfx.remote_smoke_dirty=false
        return
    end
    sdk.graphics.set_visible("vfx_smoke_remote",false)
    vfx.remote_smoke_mesh_live=false
    vfx.remote_smoke_dirty=false
end
local function new_remote_wheel(peer,short)
    return {
        key="net_"..peer.."_"..short,remote_peer=peer,front=false,
        skid_chunks={},skid_pts={},skid_meshed=0,skid_u=0,
        skid_active=false,skid_off_acc=0,skid_chunk_serial=0,smoke_accum=0,
    }
end
local function ensure_remote_wheels(peer)
    vfx.remote_wheels=vfx.remote_wheels or {}
    local list=vfx.remote_wheels[peer]
    if list then return list end
    list={new_remote_wheel(peer,"rr"),new_remote_wheel(peer,"rl")}
    vfx.remote_wheels[peer]=list
    return list
end
local function apply_wheel_vfx(w,kappa,alpha,usage,tread,gn,dt)
    local skidding=kappa>vfx.skid_kappa or alpha>vfx.skid_alpha or usage>0.68
    if not skidding or not vec(tread) or not vec(gn) then return end
    w.skidding_now=true
    if not w.skid_active then
        w.skid_active=true
        w.skid_pts={}
        w.skid_meshed=0
        reset_skid_pending(w)
    end
    push_skid_point(w,add(tread,mul(gn,vfx.skid_lift)),gn,wheel_skid_intensity(kappa,alpha,usage))
    if not w.front then
        spawn_smoke(w,add(tread,mul(gn,0.004)),wheel_smoke_intensity(kappa,alpha,usage),dt)
    end
end
local function apply_remote_wheel_vfx(w,slip,tread,gn,dt)
    local smoke_i=remote_smoke_intensity(slip)
    if smoke_i<=0 or not vec(tread) or not vec(gn) then return end
    w.skidding_now=true
    if not w.skid_active then
        w.skid_active=true
        w.skid_pts={}
        w.skid_meshed=0
        reset_skid_pending(w)
    end
    push_skid_point(w,add(tread,mul(gn,vfx.skid_lift)),gn,remote_skid_intensity(slip))
    spawn_smoke(w,add(tread,mul(gn,0.004)),smoke_i,dt)
end
local function publish_net_vfx(contacts)
    if not vfx_net_active() or not state.spawned or not sdk.net.publish then return end
    local packet={}
    for _,c in ipairs(contacts) do
        local w=c.wheel
        if w and not w.front then
            local short=w.key=="wheel_rr" and "rr" or (w.key=="wheel_rl" and "rl" or nil)
            if short then
                local slip=wheel_slip(math.abs(w.kappa or 0),math.abs(w.alpha or 0),w.usage or 0)
                if slip>=(vfx.smoke_slip_min or 0.30)*0.85 then
                    local tread=c.point
                    local gn=c.ground_n or (c.n and c.n.axis)
                    if sane_point(tread) and vec(gn) then
                        packet[short]={slip,tread[1],tread[2],tread[3],gn[1],gn[2],gn[3]}
                    end
                end
            end
        end
    end
    if next(packet) then sdk.net.publish("vfx",packet) else sdk.net.publish("vfx",nil) end
end
local function tick_remote_vfx(dt)
    if not vfx_net_active() or not vfx.supported then
        if vfx.remote_wheels and next(vfx.remote_wheels) then clear_all_remote_vfx() end
        return
    end
    local info=sdk.net.info()
    local states=((info.states or {})[sdk.mod_id] or {})
    local live={}
    local peer_count=0
    for peer,peer_states in pairs(states) do
        if tostring(peer)~=tostring(info.local_id) and type(peer_states)=="table" then
            local data=peer_states.vfx
            if type(data)=="table" and next(data) then
                peer_count=peer_count+1
                if peer_count>MAX_REMOTE_VFX_PEERS then break end
                live[peer]=true
                local wheels_list=ensure_remote_wheels(peer)
                for _,w in ipairs(wheels_list) do w.skidding_now=false end
                for short,wheel_key in pairs(REMOTE_WHEEL_KEYS) do
                    local sample=data[short]
                    if type(sample)=="table" and #sample>=7 then
                        local slip=number(sample[1],0)
                        local tread={sample[2],sample[3],sample[4]}
                        local gn={sample[5],sample[6],sample[7]}
                        local w=nil
                        for _,candidate in ipairs(wheels_list) do
                            if candidate.key=="net_"..peer.."_"..short then w=candidate; break end
                        end
                        if w then apply_remote_wheel_vfx(w,slip,tread,gn,dt) end
                    end
                end
                for _,w in ipairs(wheels_list) do
                    if w.skidding_now then
                        w.skid_off_acc=0
                    elseif w.skid_active then
                        w.skid_off_acc=(w.skid_off_acc or 0)+dt
                        if w.skid_off_acc>=0.15 then end_skid_stroke(w) end
                    end
                end
            end
        end
    end
    for peer in pairs(vfx.remote_wheels or {}) do
        if not live[peer] then clear_remote_vfx(peer) end
    end
    maybe_upload_skids()
end
local function refresh_vfx_support()
    vfx.supported=sdk.graphics and (sdk.graphics.version or 0)>=4
        and type(sdk.graphics.mesh_buffer)=="function"
        and type(sdk.graphics.mesh_buffer_append)=="function"
    return vfx.supported
end
local function ensure_vfx_mesh()
    if not refresh_vfx_support() then return false end
    if vfx.mesh then return true end
    vfx.smoke=vfx.smoke or {}
    vfx.remote_smoke=vfx.remote_smoke or {}
    vfx.remote_wheels=vfx.remote_wheels or {}
    vfx.mesh={
        smoke={positions={},normals={},colors={},uvs={},indices={}},
        remote_smoke={positions={},normals={},colors={},uvs={},indices={}},
    }
    sdk.graphics.mesh_buffer("vfx_smoke",SMOKE_BUFFER_OPTS)
    sdk.graphics.mesh_buffer("vfx_smoke_remote",SMOKE_BUFFER_OPTS)
    return true
end
local function prepare_effects()
    if not refresh_vfx_support() then return end
    clear_session_skids()
    vfx.skid_buffer_serial=0
    vfx.smoke={}; vfx.smoke_mesh_live=false; vfx.smoke_dirty=false
    vfx.remote_wheels={}; vfx.remote_smoke={}
    vfx.remote_smoke_dirty=false; vfx.remote_smoke_mesh_live=false
    vfx.mesh={
        smoke={positions={},normals={},colors={},uvs={},indices={}},
        remote_smoke={positions={},normals={},colors={},uvs={},indices={}},
    }
    sdk.graphics.mesh_buffer("vfx_smoke",SMOKE_BUFFER_OPTS)
    sdk.graphics.mesh_buffer("vfx_smoke_remote",SMOKE_BUFFER_OPTS)
    for _,w in ipairs(wheels) do
        w.skid_chunks={}
        w.skid_pts={}
        w.skid_meshed=0
        w.skid_u=0; w.skid_active=false; w.skid_off_acc=0
        w.skid_chunk_serial=0
        w.smoke_accum=0
    end
end
local function tick_wheel_vfx(contacts,dt)
    if not vfx.supported then return end
    dt=clamp(number(dt,1/60),0,0.1)
    for _,w in ipairs(wheels) do w.skidding_now=false end
    for _,c in ipairs(contacts) do
        local w=c.wheel
        local kappa=math.abs(w.kappa or 0)
        local alpha=math.abs(w.alpha or 0)
        local usage=w.usage or 0
        local tread=c.point
        local gn=c.ground_n or (c.n and c.n.axis)
        if not w.front and vec(tread) then
            sound.drift_slip=math.max(sound.drift_slip,wheel_slip(kappa,alpha,usage))
        end
        apply_wheel_vfx(w,kappa,alpha,usage,tread,gn,dt)
    end
    for _,w in ipairs(wheels) do
        if w.skidding_now then
            w.skid_off_acc=0
        elseif w.skid_active then
            w.skid_off_acc=(w.skid_off_acc or 0)+dt
            if w.skid_off_acc>=0.15 then end_skid_stroke(w) end
        end
    end
    maybe_upload_skids()
    publish_net_vfx(contacts)
end
local function upload_wheel_vfx(event)
    if not ensure_vfx_mesh() then return end
    if sdk.snapshot.paused or sdk.snapshot.replay then return end
    local dt=clamp(number(event and event.dt,1/60),0,0.1)
    tick_remote_vfx(dt)
    tick_all_smoke(dt)
    local cam=camera_position(); if not cam then return end
    if state.spawned then
        local smoke=vfx.smoke
        if not smoke or #smoke==0 then
            hide_smoke_mesh()
        elseif vfx.smoke_dirty then
            if fill_smoke_mesh(vfx.mesh.smoke,cam) then
                sdk.graphics.mesh_buffer_write("vfx_smoke",vfx.mesh.smoke)
                sdk.graphics.set_visible("vfx_smoke",true)
                vfx.smoke_mesh_live=true
                vfx.smoke_dirty=false
            elseif vfx.smoke_mesh_live then
                hide_smoke_mesh()
            end
        end
    else
        hide_smoke_mesh()
    end
    if vfx_net_active() then
        local has_remote=false
        for _,smoke in pairs(vfx.remote_smoke or {}) do
            if smoke and #smoke>0 then has_remote=true; break end
        end
        if not has_remote then
            hide_remote_smoke_mesh()
        elseif vfx.remote_smoke_dirty then
            if fill_remote_smoke_mesh(vfx.mesh.remote_smoke,cam) then
                sdk.graphics.mesh_buffer_write("vfx_smoke_remote",vfx.mesh.remote_smoke)
                sdk.graphics.set_visible("vfx_smoke_remote",true)
                vfx.remote_smoke_mesh_live=true
                vfx.remote_smoke_dirty=false
            elseif vfx.remote_smoke_mesh_live then
                hide_remote_smoke_mesh()
            end
        end
    else
        hide_remote_smoke_mesh()
        clear_all_remote_vfx()
    end
end
local function prepare_presentation()
    presentation.supported=type(sdk.camera.rig)=="function" and type(sdk.ui.canvas)=="function"
    set_debug_visible(sdk.settings.show_driving_debug==true)
    if not presentation.supported then
        sdk.log("Skyline: camera/canvas extension missing. Rebuild with the supplied Driving API source patch.")
        sdk.ui.text("skyline_presentation","Dynamic camera + speedometer require the Driving API rebuild.")
    else sdk.ui.text("skyline_presentation","") end
end
local function update_dashboard(event)
    if not presentation.supported then return end
    if not state.occupied or not state.spawned or sdk.settings.show_speedometer==false then
        if presentation.active_hud then sdk.ui.remove("driving_dashboard");presentation.active_hud=false end
        return
    end
    if sdk.snapshot.paused or sdk.snapshot.replay then return end
    local dt=clamp(number(event and event.dt,1/60),0,0.1)
    presentation.hud_timer=presentation.hud_timer-dt
    if presentation.hud_timer>0 then return end
    presentation.hud_timer=0.05
    local mph=sdk.settings.speed_mph==true
    local speed=math.floor(math.max(0,presentation.speed)*(mph and 2.236936292 or 3.6)+0.5)
    local rpm=clamp(number(state.rpm,0),0,12000)
    local amount=clamp(rpm/C.redline_rpm,0,1)
    local hot=amount>0.94
    local accent=hot and {1.0,0.27,0.18,1} or {0.23,0.79,0.98,1}
    local white={0.96,0.98,1.0,1};local muted={0.60,0.67,0.75,1}
    local items={}
    local function rect(key,x,y,w,h,color)
        items[#items+1]={key=key,type="rect",position={x,y},size={w,h},color=color}
    end
    local function text(key,x,y,w,h,value,size,color)
        items[#items+1]={key=key,type="text",position={x,y},size={w,h},text=value,font_size=size,color=color}
    end
    rect("back",0,0,344,180,{0.018,0.026,0.041,0.88})
    rect("accent",0,0,344,3,accent)
    text("title",16,12,180,20,"SKYLINE",13,muted)
    text("view",250,12,78,20,presentation.mode=="hood" and "HOOD" or "CHASE",13,muted)
    text("speed",14,31,200,78,tostring(speed),64,white)
    text("unit",180,82,68,22,mph and "MPH" or "KM/H",13,muted)
    rect("gear_box",258,39,70,68,{0.075,0.105,0.15,1})
    text("gear",277,39,47,65,gear_label(),50,white)
    text("gear_label",267,109,61,17,"GEAR",10,muted)
    text("rpm",16,115,158,22,string.format("%d RPM",math.floor(rpm/10+0.5)*10),15,white)
    text("brake",170,115,82,22,presentation.handbrake and "BRAKE" or (hot and "SHIFT" or ""),13,
        {1,0.39,0.25,1})
    rect("rpm_bg",16,144,312,9,{0.13,0.18,0.23,1})
    rect("rpm_fill",16,144,312*amount,9,accent)
    rect("redline_tick",16+312*0.94,141,2,15,{1,0.31,0.20,1})
    text("hint",16,161,312,16,"X / C CAMERA    H DEBUG    J HULL",10,muted)
    sdk.ui.canvas("driving_dashboard",{
        anchor="bottom_right",offset={24,28},size={344,180},
        scale=clamp(number(sdk.settings.dashboard_scale,1),0.65,1.5),visible=true,items=items,
    })
    presentation.active_hud=true
end
-- END DRIVING PRESENTATION

local function reset_simulation()
    state.gear=1; state.rpm=C.idle_rpm; state.steer=0; state.shift=0; state.keyboard_axis=0; state.aero_load=0
    state.engine_omega=C.idle_rpm*math.pi/30; state.clutch=0; state.mass_audit=nil
    for _,w in ipairs(wheels) do
        w.omega=0; w.load=0; w.length=C.ride_length; w.contact=false; w.slip=0
        w.spin=0; w.previous_omega=0; w.visual_steer=0; w.visual_y=0
        -- Front/rear rates match STATIC MOMENT BALANCE, not an equal-load floor.
        local fraction=w.front and ((C.center[3]+1.34703)/wheelbase)
            or ((1.44360-C.center[3])/wheelbase)
        w.static_load=C.mass*C.gravity*fraction/2
        w.stiffness=w.front and C.front_spring or C.rear_spring
        w.rest_length=C.ride_length+w.static_load/w.stiffness
        w.drive_share=(w.front and C.front_drive or 1-C.front_drive)/2
        w.alpha=0; w.kappa=0; w.fx=0; w.fy=0; w.usage=0
        w.damping=2*C.damping_ratio*math.sqrt(w.stiffness*w.static_load/C.gravity)
    end
end

-- Keep the old keys clean on reload: no orphan wheel/carrier bodies or meshes.
local function remove_rig()
    stop_audio(); clear_presentation(); clear_effects(); clear_session_skids(); clear_all_remote_vfx()
    if state.occupied or sdk.player.attached()==BODY then
        sdk.player.detach(); sdk.camera.clear_follow()
    end
    sdk.graphics.remove("skyline_visual")
    for _,w in ipairs(wheels) do
        sdk.graphics.remove(w.key)
        sdk.physics.remove(w.key)
        sdk.physics.remove("carrier_"..w.key)
        sdk.physics.remove("knuckle_"..w.key)
    end
    sdk.physics.remove(BODY)
    if state.occupied or sdk.player.attached()==BODY then
        state.enter_after=math.max(state.enter_after,state.time+C.reentry_delay)
    end
    state.spawned=false; state.occupied=false
    state.enter_requested=false; state.exit_requested=false; state.reenter_on_ready=false
end
local function ground_at(position)
    -- Search locally, not from y=500 (which could choose a roof/upper road).
    local h=sdk.physics.raycast(add(position,{0,3,0}),{0,-1,0},
        {max_distance=12,filter="ground"})
    return h and vec(h.point) and h.point[2] or nil
end
local function spawn_rig(request)
    local player=sdk.player.read()
    if not vec(player.position) then sdk.log("Skyline: player position unavailable"); return end
    local heading=request.heading or number(player.heading,0)
    local q=yaw_rotation(heading)
    local position=request.position or add(player.position,mul(rotate(q,{0,0,1}),4.5))
    local height=-math.huge
    for _,w in ipairs(wheels) do
        local off=rotate(q,w.pos)
        local y=ground_at(add(position,off))
        if not y then sdk.log("Skyline: spawn needs map ground beneath all four wheels"); return end
        height=math.max(height,y+C.radius-off[2]+0.025)
    end
    sdk.physics.spawn(BODY,{
        shape={type="model",path=MODEL,object="skyline_mesh",
            options={max_hulls=32,resolution=96,concavity=0.0025}},
        body_type="dynamic",mass=C.mass,position={position[1],height,position[3]},
        heading=heading,friction=0.25,ccd=true,
        -- Native assembly classification; the SDK treats this as metadata.
        contact_group=8,
        center_of_mass=C.center,inertia_half_extents=C.inertia_half,
        linear_damping=0,angular_damping=0,
    })
    sdk.graphics.mesh("skyline_visual",{path=MODEL,body=BODY})
    prepare_effects()
    reset_simulation()
    state.spawned=true; state.enter_after=math.max(state.enter_after,state.time+0.25)
    start_audio()
    state.reenter_on_ready=request.reenter==true
end
local function respawn(car,at_car)
    local request={}
    if at_car and car then
        request={position=car.position,heading=yaw(car.rotation),reenter=state.occupied}
    end
    remove_rig()
    -- Spawn on the next tick: queries cannot see this tick's queued spawns.
    state.pending=request
end

local function input_state(buttons,dt)
    local p=sdk.input.pad(); local t=p.triggers or {}; local stick=p.left or {}
    local up=edge("KeyX") or pressed_button(buttons,0x0200)
    local dn=edge("KeyZ") or pressed_button(buttons,0x0100)
    local neutral=edge("KeyN")
    local throttle,brake,steer,hand=0,1,0,false
    if state.occupied then
        if neutral then state.gear=0; state.shift=C.shift_time
        elseif up and state.gear<6 then state.gear=state.gear+1; state.shift=C.shift_time
        elseif dn and state.gear> -1 then state.gear=state.gear-1; state.shift=C.shift_time end
        throttle=math.max(deadzone(clamp(number(t[2],0),0,1),0.04),down("KeyW") and 1 or 0)
        brake=math.max(deadzone(clamp(number(t[1],0),0,1),0.04),down("KeyS") and 1 or 0)
        local digital=(down("KeyD") and 1 or 0)-(down("KeyA") and 1 or 0)
        state.keyboard_axis=approach(state.keyboard_axis,digital,dt/(digital==0 and 0.22 or 0.45))
        steer=clamp(deadzone(clamp(number(stick[1],0),-1,1),0.09)+state.keyboard_axis,-1,1)
        hand=down("ShiftLeft") or down("ShiftRight") or (buttons & 0x2000)~=0
    end
    state.shift=math.max(0,state.shift-dt)
    return {throttle=throttle,brake=brake,steer=steer,hand=hand,raw_rt=number(t[2],0)}
end
local function steering_angles(input,dt,speed)
    -- Driver-input mapping, NOT steering assistance or a tire/force correction.
    -- More precision near stick centre at speed; FULL +/-40 deg remains available.
    -- No measured/target sideslip or yaw rate is used. Zero input requests zero.
    local blend=clamp((number(speed,0)-15)/40,0,1)
    blend=blend*blend*(3-2*blend)
    local precision=clamp(number(sdk.settings.steering_precision,1),0,1.5)
    local exponent=C.steering_exponent+1.25*precision*blend
    local shaped=(input<0 and -1 or 1)*math.abs(input)^exponent
    state.steer=approach(state.steer,-shaped*C.steer_limit,C.steer_rate*dt)
    if math.abs(state.steer)<1e-6 then return {0,0,0,0} end
    local radius=wheelbase/math.tan(math.abs(state.steer))
    local inner=math.atan(wheelbase/(radius-track/2))
    local outer=math.atan(wheelbase/(radius+track/2))
    if state.steer<0 then return {-outer,-inner,0,0} end
    return {inner,outer,0,0}
end
local function torque_curve(rpm)
    local points={{900,185},{2000,270},{3500,365},{5000,405},{6500,395},{8000,320}}
    for i=2,#points do
        if rpm<=points[i][1] then
            local a,b=points[i-1],points[i]
            return a[2]+(b[2]-a[2])*clamp((rpm-a[1])/(b[1]-a[1]),0,1)
        end
    end
    return points[#points][2]
end
local function prepare_powertrain(input,dt,speed)
    -- A finite-torque engine. Idle is a torque controller, NOT an RPM floor.
    local rpm=state.engine_omega*30/math.pi
    local loss=12+0.025*math.abs(state.engine_omega)
        +(1-input.throttle)*35*clamp((rpm-C.idle_rpm)/2200,0,1)
    local fuel=clamp((C.redline_rpm-rpm)/350,0,1)
    local idle=clamp(loss+0.9*(C.idle_rpm-rpm),0,100)
    local combustion=math.max(input.throttle*torque_curve(rpm)*fuel,idle)
    local net=combustion-loss
    state.engine_omega=math.max(0,state.engine_omega+net*dt/C.engine_inertia)
    -- Automatic clutch pedal; it does not look at tire slip or sideslip.
    local wanted=clamp((rpm-1000)/1100,0,1)
    if speed>4 and rpm>1000 then wanted=1 end
    if (speed<1 and input.throttle==0) or state.shift>0 or state.gear==0 then wanted=0 end
    state.clutch=approach(state.clutch,wanted,(wanted<state.clutch and 10 or 5)*dt)
    if state.shift>0 or state.gear==0 then state.clutch=0 end
    local ratio=gear_ratio()
    local inverse=1/C.engine_inertia
    for _,w in ipairs(wheels) do
        w.shaft_factor=ratio*w.drive_share
        inverse=inverse+w.shaft_factor*w.shaft_factor/C.wheel_inertia
    end
    return {p=0, cap=C.clutch_capacity*state.clutch*dt,
        inverse=inverse, diff_p=0, ratio=ratio}
end
local function solve_powertrain(power,dt)
    -- Projected passive clutch impulse in gearbox coordinates.
    local relative=state.engine_omega
    for _,w in ipairs(wheels) do relative=relative-w.shaft_factor*w.omega end
    local next_p=clamp(power.p+relative/power.inverse,-power.cap,power.cap)
    local change=next_p-power.p
    state.engine_omega=state.engine_omega-change/C.engine_inertia
    for _,w in ipairs(wheels) do w.omega=w.omega+w.shaft_factor*change/C.wheel_inertia end
    power.p=next_p
    -- A torque-limited rear clutch LSD, not a forced equal-speed axle.
    -- Brake/drive force remains entirely a tire result. Front diff is open.
    local rear_torque=math.abs(power.p*power.ratio*(1-C.front_drive)/dt)
    local cap=(C.rear_diff_preload+C.rear_diff_ramp*rear_torque)*dt
    local delta=wheels[3].omega-wheels[4].omega
    local next_diff=clamp(power.diff_p-delta*C.wheel_inertia/2,-cap,cap)
    local dp=next_diff-power.diff_p
    wheels[3].omega=wheels[3].omega+dp/C.wheel_inertia
    wheels[4].omega=wheels[4].omega-dp/C.wheel_inertia
    power.diff_p=next_diff
end

-- Invert a small SPD matrix (1..4 contacts), with partial pivoting.
local function inverse_matrix(matrix)
    local n=#matrix; local a={}
    for i=1,n do
        a[i]={}
        for j=1,n do a[i][j]=matrix[i][j]; a[i][n+j]=(i==j and 1 or 0) end
    end
    for k=1,n do
        local pivot=k
        for i=k+1,n do if math.abs(a[i][k])>math.abs(a[pivot][k]) then pivot=i end end
        if math.abs(a[pivot][k])<1e-12 then return nil end
        a[k],a[pivot]=a[pivot],a[k]
        local d=a[k][k]
        for j=1,2*n do a[k][j]=a[k][j]/d end
        for i=1,n do if i~=k then
            local factor=a[i][k]
            for j=1,2*n do a[i][j]=a[i][j]-factor*a[k][j] end
        end end
    end
    local result={}
    for i=1,n do result[i]={}; for j=1,n do result[i][j]=a[i][n+j] end end
    return result
end
local function constraint_axis(q,arm,axis)
    local torque=cross(arm,axis)
    local angular=inertia(q,torque,true)
    return {axis=axis,torque=torque,angular=angular,inv_mass=1/C.mass+dot(torque,angular)}
end
local function velocity_along(sim,c)
    return dot(sim.v,c.axis)+dot(sim.w,c.torque)
end
local function push(sim,c,impulse)
    for i=1,3 do
        sim.v[i]=sim.v[i]+c.axis[i]*impulse/C.mass
        sim.w[i]=sim.w[i]+c.angular[i]*impulse
    end
end
local function collect_contacts(car,angles,dt)
    local result={}; local by_wheel={}
    local q=car.rotation; local up=rotate(q,{0,1,0}); local direction=mul(up,-1)
    local center=add(car.position,rotate(q,C.center))
    for i,w in ipairs(wheels) do
        w.contact=false; w.load=0; w.slip=0; w.length=C.droop_length
        w.alpha=0; w.kappa=0; w.fx=0; w.fy=0; w.usage=0
        local local_mount={w.pos[1],w.pos[2]+C.ride_length,w.pos[3]}
        local mount=add(car.position,rotate(q,local_mount))
        local origin=add(mount,mul(up,0.10))
        -- Ground-only is intentional: ignores our chassis, skater proxies,
        -- and other dynamic objects. Dynamic-prop driving is not implemented.
        local hit=sdk.physics.raycast(origin,direction,
            {max_distance=0.10+C.droop_length+C.radius,filter="ground"})
        if hit and vec(hit.point) and vec(hit.normal) then
            local n=norm(hit.normal)
            local alignment=dot(n,up)
            local distance=dot(sub(hit.point,mount),direction)
            local length=distance-C.radius/math.max(0.45,alignment)
            if alignment>0.45 and n[2]>0.1 and length<=C.droop_length then
                local forward=rotate(q,{math.sin(angles[i]),0,math.cos(angles[i])})
                forward=norm(sub(forward,mul(n,dot(forward,n))))
                local lateral=norm(cross(n,forward))
                -- Spherical contact-envelope approximation; normal force acts ON the road.
                local hub=sub(mount,mul(up,length))
                local point=sub(hub,mul(n,C.radius))
                local arm=sub(point,center)
                local c={wheel=w,wheel_index=i,point=point,ground=hit.point,ground_n=n,
                    alignment=alignment,length=length,compression=w.rest_length-length,
                    pn=0,px=0,py=0,pb=0,
                    n=constraint_axis(q,arm,n),
                    x=constraint_axis(q,arm,forward),y=constraint_axis(q,arm,lateral)}
                c.xy=dot(c.x.axis,c.y.axis)/C.mass+dot(c.x.torque,c.y.angular)
                result[#result+1]=c; by_wheel[i]=#result
                w.contact=true; w.length=length
            end
        end
    end
    if #result==0 then return result end
    -- Backward Euler for ALL corner springs + axle anti-roll springs:
    -- (W + Gamma) p = beta - v_free; p is unilateral and force-limited.
    -- Gamma = [dt A^-1(D + dt K)A^-1]^-1, beta=Gamma*dt*A^-1*K*x.
    -- Axle couplings make anti-roll torque equal/opposite, not free downforce.
    local matrix,bias={},{ }
    for i,c in ipairs(result) do
        local partner=by_wheel[c.wheel.mate]
        local antiroll=c.wheel.front and C.front_antiroll or C.rear_antiroll
        local k=c.wheel.stiffness
        local force=k*c.compression
        local bump=math.max(0,C.bump_length-c.length)
        if bump>0 then k=k+C.bump_stiffness; force=force+C.bump_stiffness*bump end
        local d=c.wheel.damping
        if partner then
            k=k+antiroll; d=d+C.antiroll_damping
            force=force+antiroll*(c.compression-result[partner].compression)
        end
        matrix[i]={}
        for j,other in ipairs(result) do
            local value=0
            if i==j then value=d+dt*k
            elseif j==partner then value=-C.antiroll_damping-dt*antiroll end
            matrix[i][j]=dt*value/(c.alignment*other.alignment)
        end
        bias[i]=dt*force/c.alignment
    end
    local gamma=inverse_matrix(matrix)
    if not gamma then return {} end
    for i,c in ipairs(result) do
        c.gamma=gamma[i]; c.beta=0
        for j=1,#result do c.beta=c.beta+gamma[i][j]*bias[j] end
    end
    return result
end
local function solve_normal(sim,contacts,index,dt)
    local c=contacts[index]
    local residual=velocity_along(sim,c.n)-c.beta
    for j,other in ipairs(contacts) do residual=residual+c.gamma[j]*other.pn end
    local new=clamp(c.pn-residual/(c.n.inv_mass+c.gamma[index]),0,C.maximum_load*dt)
    push(sim,c.n,new-c.pn); c.pn=new
end
-- Solve the tire contact for a trial scalar compliance. Brake complementarity
-- is solved analytically: first test omega=0, otherwise use a saturated brake.
-- vx0,vy0,omega0 have THIS contact's accumulated impulses removed.
local function tire_candidate(c,g,vx0,vy0,omega0,brake)
    local a,b,d=c.x.inv_mass+g,c.xy,c.y.inv_mass+g
    local determinant=a*d-b*b
    local px=(-d*vx0+b*vy0)/determinant
    local py=(b*vx0-a*vy0)/determinant
    local required=C.radius*px-C.wheel_inertia*omega0
    if math.abs(required)<=brake then return px,py,required,0 end
    local pb=clamp(required,-brake,brake)
    local free_omega=omega0+pb/C.wheel_inertia
    local sx=vx0-C.radius*free_omega
    a=a+C.radius*C.radius/C.wheel_inertia
    determinant=a*d-b*b
    px=(-d*sx+b*vy0)/determinant
    py=(b*sx-a*vy0)/determinant
    return px,py,pb,free_omega-C.radius*px/C.wheel_inertia
end
local function brush_inverse_scale(ratio)
    -- Inverse of F/D = 1 - (1 - demand/(3D))^3.
    -- Rationalized to avoid cancellation as force approaches zero.
    local root=(1-clamp(ratio,0,1))^(1/3)
    return 3/(1+root+root*root)
end
local function solve_tire(sim,c,input,mu,dt)
    local w=c.wheel
    local oldx,oldy=c.px,c.py
    local vx,vy=velocity_along(sim,c.x),velocity_along(sim,c.y)
    local vx0=vx-c.x.inv_mass*oldx-c.xy*oldy
    local vy0=vy-c.xy*oldx-c.y.inv_mass*oldy
    local omega0=w.omega+(C.radius*oldx-c.pb)/C.wheel_inertia
    local load=c.pn/dt
    local load_ratio=math.max(0,load/w.static_load)
    -- Sublinear load sensitivity, with bounded low-load coefficient.
    local sensitivity=clamp(math.max(load_ratio,0.01)^(C.load_exponent-1),0.65,1.15)
    local limit=mu*sensitivity*c.pn
    local fraction=w.front and C.front_brake_fraction or 1-C.front_brake_fraction
    local brake=(input.brake*C.brake_torque_total*fraction/2
        +C.rolling_coefficient*load*C.radius
        +((input.hand and not w.front) and C.handbrake_torque or 0))*dt
    local px,py,pb,omega=0,0,clamp(-C.wheel_inertia*omega0,-brake,brake),0
    omega=omega0+pb/C.wheel_inertia
    if limit>1e-8 then
        local transport=math.max(math.abs(vx),math.abs(w.omega*C.radius))
        -- Static-friction limit near rest. Smooth transition to rolling brush.
        -- This is local tire-contact regularization, never a chassis assist.
        local blend=clamp((transport-0.15)/0.85,0,1)
        blend=blend*blend*(3-2*blend)
        local stiffness=(w.front and C.front_stiffness_per_load or C.rear_stiffness_per_load)
            *w.static_load*math.max(load_ratio,0.01)^C.load_exponent
        local base=blend*math.max(transport,0.15)/(stiffness*dt)
        local low,high=base,3*base
        px,py,pb,omega=tire_candidate(c,high,vx0,vy0,omega0,brake)
        local amount=math.sqrt(px*px+py*py)
        if base==0 and amount<=limit then
            -- True sticking branch; a parked vehicle can hold a slope.
        else
            local sliding=amount>limit
            if sliding then
                low=high
                high=high+(math.sqrt(vx0*vx0+vy0*vy0)+C.radius*math.abs(omega0))/limit+1e-6
            end
            for _=1,C.tire_root_iterations do
                local g=(low+high)/2
                local tx,ty=tire_candidate(c,g,vx0,vy0,omega0,brake)
                local magnitude=math.sqrt(tx*tx+ty*ty)
                local increase
                if sliding then increase=magnitude>limit
                else increase=g<base*brush_inverse_scale(magnitude/limit) end
                if increase then low=g else high=g end
            end
            px,py,pb,omega=tire_candidate(c,high,vx0,vy0,omega0,brake)
        end
    end
    w.omega=omega
    push(sim,c.x,px-oldx); push(sim,c.y,py-oldy)
    c.px=px; c.py=py; c.pb=pb; c.limit=limit
end
local function publish_hud(car,input,contacts,dt)
    state.hud_time=state.hud_time-dt
    if state.hud_time>0 then return end
    state.hud_time=0.10
    if not presentation.debug then
        sdk.ui.text("skyline_status",""); sdk.ui.text("skyline_handling",""); sdk.ui.text("skyline_wheels",""); return
    end
    local velocity=unrotate(car.rotation,car.linvel)
    local local_up=unrotate(car.rotation,{0,1,0})
    local roll=math.deg(math.atan(local_up[1],local_up[2]))
    local beta=math.deg(math.atan(velocity[1],math.max(0.1,math.abs(velocity[3]))))
    local loaded=0
    for _,w in ipairs(wheels) do if w.load>1 then loaded=loaded+1 end end
    sdk.ui.text("skyline_status",string.format(
        "Skyline DRIVE | %+.1f km/h | %s | %.0f rpm | T %.0f%% Brake %.0f%% HB %s",
        velocity[3]*3.6,gear_label(),state.rpm,input.throttle*100,input.brake*100,
        input.hand and "ON" or "off"))
    sdk.ui.text("skyline_handling",string.format(
        "Rack %+.1f deg | beta %+.1f deg | roll %+.1f deg | mu %.2f | load %d/4 | mass %s | aero %.0fN",
        math.deg(state.steer),beta,roll,number(sdk.settings.tire_friction,1.3),loaded,
        state.mass_audit or "pending",state.aero_load))
    sdk.ui.text("skyline_wheels",string.format(
        "FL %.0fN a%+.1f k%+.2f | FR %.0fN a%+.1f k%+.2f | RL %.0fN a%+.1f k%+.2f | RR %.0fN a%+.1f k%+.2f | clutch %.0f%%",
        wheels[1].load,wheels[1].alpha,wheels[1].kappa,wheels[2].load,wheels[2].alpha,wheels[2].kappa,
        wheels[3].load,wheels[3].alpha,wheels[3].kappa,wheels[4].load,wheels[4].alpha,wheels[4].kappa,state.clutch*100))
end
-- Read-only consistency check against NATIVE effective mass. Snapshot `mass`
-- is metadata in this engine, so checking that field alone proves nothing.
-- This never changes forces, inertia, pose, or velocity to hide a mismatch.
local function audit_mass_properties(car)
    if state.mass_audit~=nil then return end
    local q=car.rotation
    local center=add(car.position,rotate(q,C.center))
    local probes={
        {{0,0,0},{0,1,0},1/C.mass},
        {{0,1,0},{0,0,1},1/C.mass+1/I[1]},
        {{0,0,1},{1,0,0},1/C.mass+1/I[2]},
        {{1,0,0},{0,1,0},1/C.mass+1/I[3]},
    }
    local error=0
    for _,probe in ipairs(probes) do
        local actual=sdk.physics.effective_inv_mass(BODY,
            add(center,rotate(q,probe[1])),rotate(q,probe[2]))
        if not finite(actual) then
            state.mass_audit="unavailable"
            sdk.log("Skyline: native mass check unavailable; supplied-source mass properties assumed")
            return
        end
        error=math.max(error,math.abs(actual-probe[3])/probe[3])
    end
    state.mass_audit=error<0.01 and "OK" or "MISMATCH"
    sdk.log(string.format("Skyline: native mass/inertia check %s (maximum relative error %.3f%%)",
        state.mass_audit,100*error))
end
-- Explicit aerodynamic surface loads. Cl*A values are editable ASSUMPTIONS,
-- not measured R34 data. Positive values mean downforce, negative mean lift.
-- Forces act at fore/aft locations and enter the existing coupled solver.
-- No contact-count condition, global-down force, or tire-friction multiplier.
local function apply_aerodynamics(sim,car,dt)
    state.aero_load=0
    if sdk.settings.aero_enabled==false then return end
    local front=clamp(number(sdk.settings.aero_front_area,0.08),-0.4,0.6)
    local rear=clamp(number(sdk.settings.aero_rear_area,0.12),-0.4,0.6)
    local up=rotate(car.rotation,{0,1,0})
    local forward=rotate(car.rotation,{0,0,1})
    local surface_positions={{0,0.04,1.44360},{0,0.16,-1.34703}}
    -- Evaluate both surfaces at the same state; avoid update-order dependence.
    local air_linear,air_angular=sim.v,sim.w
    for i,area in ipairs({front,rear}) do
        local arm=rotate(car.rotation,sub(surface_positions[i],C.center))
        local velocity=add(air_linear,cross(air_angular,arm))
        local speed2=dot(velocity,velocity)
        if speed2>0.01 and area~=0 then
            local along=dot(velocity,forward)
            -- Project body-down perpendicular to local airflow: lift does not
            -- deliberately add thrust or oppose the body's roll orientation.
            local flow=mul(velocity,1/math.sqrt(speed2))
            local direction=sub(mul(up,-1),mul(flow,dot(mul(up,-1),flow)))
            local projected2=dot(direction,direction)
            if projected2>1e-8 then
                local force=mul(direction,(0.5*1.225*along*along*area)/math.sqrt(projected2))
                local impulse=mul(force,dt)
                sim.v=add(sim.v,mul(impulse,1/C.mass))
                sim.w=add(sim.w,inertia(car.rotation,cross(arm,impulse),true))
                state.aero_load=state.aero_load-dot(force,up)
            end
        end
    end
end

local function quat_mul(a,b)
    return {
        a[4]*b[1]+a[1]*b[4]+a[2]*b[3]-a[3]*b[2],
        a[4]*b[2]-a[1]*b[3]+a[2]*b[4]+a[3]*b[1],
        a[4]*b[3]+a[1]*b[2]-a[2]*b[1]+a[3]*b[4],
        a[4]*b[4]-a[1]*b[1]-a[2]*b[2]-a[3]*b[3],
    }
end
local function update_wheel_visuals(angles,dt)
    for i,w in ipairs(wheels) do
        -- The solver owns omega and suspension length. These are read-only
        -- presentation values: no extra rigid body, force or steering assist.
        w.spin=(w.spin+0.5*(w.previous_omega+w.omega)*dt)%(2*math.pi)
        local steer=angles[i]
        local y=C.ride_length-w.length
        local steer_q=yaw_rotation(steer)
        local spin_q={math.sin(w.spin/2),0,0,math.cos(w.spin/2)}
        local axle=rotate(steer_q,{w.omega,0,0})
        sdk.graphics.node_transform("skyline_visual",w.key,{
            position={0,y,0}, rotation=quat_mul(steer_q,spin_q), relative=true,
            linear_velocity={0,(y-w.visual_y)/dt,0},
            angular_velocity={axle[1],(steer-w.visual_steer)/dt,axle[3]},
        })
        w.previous_omega=w.omega; w.visual_y=y; w.visual_steer=steer
    end
end
local function interaction_text(text)
    if state.interaction_text~=text then
        state.interaction_text=text; sdk.ui.text("skyline_interaction",text)
    end
end
local function near_door(car)
    local player=sdk.player.read()
    if not vec(player.position) then return false end
    local p=unrotate(car.rotation,sub(player.position,car.position))
    if p[2]<-1.3 or p[2]>1.5 then return false end
    for _,side in ipairs({-1,1}) do
        local dx,dz=p[1]-side*1.65,p[3]+0.20
        if dx*dx+dz*dz <= C.entry_radius*C.entry_radius then return true end
    end
    return false
end
local function request_entry()
    if state.enter_requested or sdk.player.detaching() or state.time<state.enter_after then return end
    sdk.player.attach(BODY,{-0.38,0.12,-0.23})
    state.enter_requested=true; state.reenter_on_ready=false
end
local function request_exit()
    if state.exit_requested then return end
    -- Body-local FOOT candidates. The engine rotates them with the chassis,
    -- finds map ground, and rejects occupied full-height standing capsules.
    sdk.player.detach({candidates={
        {-1.75,-0.52,-0.20},{1.75,-0.52,-0.20},
        {-1.90,-0.52,1.3},{1.90,-0.52,1.3},
        {-1.90,-0.52,-1.5},{1.90,-0.52,-1.5},
        {0,-0.52,3.25},{0,-0.52,-3.15},
    },height=1.8,radius=0.30,ground_snap=3.0})
    state.exit_requested=true
end
local function sync_occupancy()
    local attached=sdk.player.attached()==BODY
    if attached then
        state.enter_requested=false
        if not state.occupied then state.occupied=true; configure_camera() end
        local err=sdk.player.detach_error()
        if state.exit_requested and type(err)=="string" then
            state.exit_requested=false; interaction_text(err)
        end
    else
        if state.occupied then
            state.occupied=false; state.exit_requested=false
            state.enter_after=math.max(state.enter_after,state.time+C.reentry_delay)
            clear_presentation()
            if presentation.debug then set_debug_visible(true) end
        end
        state.enter_requested=false
    end
end
local function set_collision_debug(enabled)
    state.hull_debug=enabled==true
    sdk.physics.debug_colliders(state.hull_debug)
end

local function simulate(car,input,dt)
    if not vec(car.linvel) or not vec(car.angvel) or not vec(car.position)
        or type(car.rotation)~="table" or not finite(car.rotation[4]) then return end
    audit_mass_properties(car)
    local speed=math.sqrt(car.linvel[1]^2+car.linvel[3]^2)
    local angles=steering_angles(input.steer,dt,speed)
    local power=prepare_powertrain(input,dt,speed)
    local contacts=collect_contacts(car,angles,dt)
    -- Gravity is integrated by DynamicsWorld with the submitted forces.
    -- Include it in the prediction, but DO NOT submit gravity a second time.
    local free={car.linvel[1],car.linvel[2]-C.gravity*dt,car.linvel[3]}
    local sim={v={free[1],free[2],free[3]},w={car.angvel[1],car.angvel[2],car.angvel[3]}}
    local drag=1/(1+C.aero_drag*speed*dt/C.mass)
    sim.v[1]=sim.v[1]*drag; sim.v[3]=sim.v[3]*drag
    apply_aerodynamics(sim,car,dt)
    local mu=clamp(number(sdk.settings.tire_friction,1.3),0.4,2)
    -- Alternating sweep order avoids a persistent front/rear solver bias.
    for pass=1,C.iterations do
        solve_powertrain(power,dt)
        for k=1,#contacts do
            local index=(pass%2==1) and k or (#contacts-k+1)
            solve_normal(sim,contacts,index,dt)
        end
        for k=1,#contacts do
            local index=(pass%2==1) and k or (#contacts-k+1)
            solve_tire(sim,contacts[index],input,mu,dt)
        end
    end
    local body_center=add(car.position,rotate(car.rotation,C.center))
    for _,c in ipairs(contacts) do
        c.wheel.load=c.pn/dt
        local vx,vy=velocity_along(sim,c.x),velocity_along(sim,c.y)
        local tread=c.wheel.omega*C.radius
        c.wheel.slip=tread-vx
        c.wheel.kappa=(tread-vx)/math.max(math.abs(vx),math.abs(tread),0.5)
        c.wheel.alpha=math.deg(math.atan(vy,math.max(math.abs(vx),0.1)))
        c.wheel.fx=c.px/dt; c.wheel.fy=c.py/dt
        c.wheel.usage=c.limit and math.sqrt(c.px*c.px+c.py*c.py)/math.max(c.limit,1e-8) or 0
        local arm=sub(c.ground or c.point,body_center)
        c.surface_v=add(sim.v,cross(sim.w,arm))
    end
    for _,w in ipairs(wheels) do if not w.contact then
        local fraction=w.front and C.front_brake_fraction or 1-C.front_brake_fraction
        local brake=input.brake*C.brake_torque_total*fraction/2
            +((input.hand and not w.front) and C.handbrake_torque or 0)
        w.omega=approach(w.omega,0,brake*dt/C.wheel_inertia)
    end end
    local impulse=mul(sub(sim.v,free),C.mass)
    local angular=inertia(car.rotation,sub(sim.w,car.angvel),false)
    if vec(impulse) and vec(angular) then
        -- apply_force/apply_torque each REPLACE the previous force/torque
        -- in this engine. Aggregate here, then submit once per body per tick.
        sdk.physics.force(BODY,mul(impulse,1/dt))
        sdk.physics.torque(BODY,mul(angular,1/dt))
    end
    state.rpm=state.engine_omega*30/math.pi
    sound.throttle=input.throttle; sound.shift=state.shift; sound.brake=input.brake
    local body_velocity=unrotate(car.rotation,car.linvel)
    state.speed=math.sqrt(body_velocity[1]^2+body_velocity[3]^2)
    presentation.speed=state.speed
    presentation.handbrake=input.hand
    publish_hud(car,input,contacts,dt)
    sound.drift_slip=0
    tick_wheel_vfx(contacts,dt)
    update_wheel_visuals(angles,dt)
end

return {
    on_load=function()
        -- Native capabilities are published by vm.rs, not invented by api.lua.
        -- Replacing only the Lua wrapper must NOT enable unknown native commands.
        local native=sdk.capabilities or {}
        if (native.model_collision or 0)<1 or (native.solid_bridge or 0)<3
            or (native.physics_debug or 0)<1 or (native.scene_transforms or 0)<2 then
            error("Skyline 4.3.1: the running executable is missing the native Model Collision Repair API. "..
                "Install the complete matching crates update and launch a successfully rebuilt game executable. "..
                "Copying only api.lua is not sufficient.")
        end
        if not sdk.graphics or (sdk.graphics.version or 0)<2 or not sdk.player.detaching then
            error("Skyline 4.3.1: the embedded Lua SDK wrapper does not match the native Model Collision Repair API")
        end
        reset_simulation(); remove_rig(); prepare_audio(); prepare_presentation(); ensure_vfx_mesh()
        set_collision_debug(sdk.settings.show_collision_hull==true)
        sdk.log("Skyline 4.3.1: render-model compound collision, native impacts, animated wheels, safe exits; J toggles actual colliders")
        sdk.ui.text("skyline_status",""); sdk.ui.text("skyline_handling",""); sdk.ui.text("skyline_wheels","")
    end,
    on_update=function(event)
        update_audio(event); update_dashboard(event); upload_wheel_vfx(event)
        refresh_mp_status(clamp(number(event and event.dt,1/60),0,0.1))
    end,
    on_settings=function(event)
        if event.key=="audio_enabled" and event.value==false then stop_audio() end
        if event.key=="show_driving_debug" then set_debug_visible(event.value==true) end
        if event.key=="show_collision_hull" then set_collision_debug(event.value==true) end
        if event.key=="show_speedometer" and event.value==false and presentation.supported then
            sdk.ui.remove("driving_dashboard");presentation.active_hud=false
        end
        presentation.hud_timer=0
        if event.key:match("^camera_") or event.key=="hood_height" then configure_camera() end
    end,
    on_unload=function()
        set_collision_debug(false); interaction_text(""); remove_rig()
        clear_session_skids()
        sdk.ui.text("skyline_audio",""); sdk.ui.text("skyline_presentation","")
        sdk.ui.text("skyline_help",""); sdk.ui.text("skyline_status",""); sdk.ui.text("skyline_handling",""); sdk.ui.text("skyline_wheels","")
        sdk.ui.text("skyline_mp","")
    end,
    on_fixed_update=function(event)
        local dt=number(event and event.dt,1/120)
        if dt<=0 or dt>0.1 then return end
        state.time=state.time+dt
        sync_occupancy()
        local p=sdk.input.pad(); local buttons=math.floor(number(p.buttons,0))
        -- Sample edges unconditionally; held keys must not become new presses
        -- merely because another shortcut returned early this tick.
        local spawn=edge("F10")
        local reset_key=edge("KeyR")
        local enter_key=edge("KeyE")
        local camera_key=edge("KeyC")
        local debug_key=edge("KeyH")
        local hull_key=edge("KeyJ")
        if hull_key then set_collision_debug(not state.hull_debug) end
        local camera_pressed=camera_key or pressed_button(buttons,0x4000)
        if debug_key then set_debug_visible(not presentation.debug) end
        local enter=enter_key or pressed_button(buttons,0x8000)
        local reset=reset_key or pressed_button(buttons,0x0080)
        local car=sdk.physics.read(BODY)
        if spawn then respawn(nil,false)
        elseif state.pending then
            local request=state.pending; state.pending=nil; spawn_rig(request)
        elseif car and state.spawned then
            if reset then respawn(car,true)
            else
                if state.reenter_on_ready and state.time>=state.enter_after and not sdk.player.detaching() then
                    request_entry()
                end
                if enter then
                    if state.occupied then request_exit()
                    elseif state.time>=state.enter_after and near_door(car) then request_entry() end
                end
                if not state.occupied then
                    local remaining=math.max(0,state.enter_after-state.time)
                    if sdk.player.detaching() then interaction_text("Moving to a clear on-foot exit...")
                    elseif remaining>0 then interaction_text(string.format("Re-entry: %.1f s | J: collision hull",remaining))
                    elseif near_door(car) then interaction_text("E / Y: enter | J: collision hull")
                    else interaction_text("Approach a door to enter | J: collision hull") end
                elseif not state.exit_requested and type(sdk.player.detach_error())~="string" then
                    interaction_text("E / Y: exit | J: collision hull")
                end
                if camera_pressed and state.occupied then
                    presentation.mode=presentation.mode=="chase" and "hood" or "chase"
                    configure_camera();presentation.hud_timer=0
                end
                simulate(car,input_state(buttons,dt),dt)
            end
        end
        state.buttons=buttons
    end,
    on_event=function(event)
        if event.name=="world_changed" then
            remove_rig(); stop_audio(); clear_presentation(); sound.prepared=false; prepare_audio()
            state.pending=nil
            state.enter_requested=false; state.exit_requested=false; state.reenter_on_ready=false
            state.enter_after=state.time+C.reentry_delay; interaction_text("")
            set_collision_debug(state.hull_debug)
            state.keys={}; state.buttons=0; reset_simulation()
            sdk.ui.text("skyline_status",""); sdk.ui.text("skyline_handling",""); sdk.ui.text("skyline_wheels","")
            sdk.ui.text("skyline_mp","")
        end
    end,
}
