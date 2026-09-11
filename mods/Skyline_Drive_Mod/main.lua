-- Driving edition 1: latest working vehicle + unchanged audio; render-rate cameras,
-- retained dashboard, progressive driver-input mapping, explicit optional aero.
-- Skyline Physics v4 -- Lua 5.4 / supplied Skate SDK API 2.
-- Reduced-order vehicle: one 6-DOF chassis, four rotating tire DOFs,
-- passive clutch/differential/brakes, and a combined-slip brush tire law.
-- No yaw targets, countersteer assist, slide-recovery state, lateral-velocity
-- deletion, handbrake grip multiplier, or upright torque. Aero is an explicit load.
-- See README.md and PHYSICS_AND_API.md for changes and declared assumptions.

local BODY, MODEL = "chassis", "skyline.glb"
local C = {
    mass = 1400, gravity = 9.81,
    -- At nominal ride height: CG is 0.400 m above a level road.
    -- This is an explicitly tuned chassis, NOT measured factory Skyline data.
    center = {0, -0.27160, 0.16},
    inertia_half = {0.92, 0.55, 2.10},
    hull_half = {0.86, 0.30, 2.04},
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
}

local function clamp(x,a,b) return math.max(a,math.min(b,x)) end
local function finite(x) return type(x)=="number" and x==x and math.abs(x)<math.huge end
local function number(x, fallback) return finite(x) and x or fallback end
local function vec(x) return type(x)=="table" and finite(x[1]) and finite(x[2]) and finite(x[3]) end
local function add(a,b) return {a[1]+b[1],a[2]+b[2],a[3]+b[3]} end
local function sub(a,b) return {a[1]-b[1],a[2]-b[2],a[3]-b[3]} end
local function mul(a,s) return {a[1]*s,a[2]*s,a[3]*s} end
local function dot(a,b) return a[1]*b[1]+a[2]*b[2]+a[3]*b[3] end
local function cross(a,b) return {a[2]*b[3]-a[3]*b[2],a[3]*b[1]-a[1]*b[3],a[1]*b[2]-a[2]*b[1]} end
local function norm(a)
    local l=math.sqrt(dot(a,a))
    return l>1e-8 and mul(a,1/l) or {0,0,0}
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
-- AUDIO EXTENSION: presentation only. Never writes RPM, wheel speed, or forces.
local sound = {
    running=false, supported=false, prepared=false, clock=0, cooldown=0,
    rpm=900, throttle=0, shift=0, previous_throttle=0,
    overrun_remaining=0, pop_slot=0, random_state=73641, chop_until=0,
}
local sound_anchors={900,2400,4200,6600}
local function sound_random()
    sound.random_state=(sound.random_state*48271)%2147483647
    return sound.random_state/2147483647
end
local function stop_audio()
    if sound.supported then sdk.audio.stop_all() end
    sound.running=false; sound.cooldown=0; sound.overrun_remaining=0
    sound.previous_throttle=0; sound.chop_until=0
end
local function prepare_audio()
    sound.supported=type(sdk.audio)=="table" and number(sdk.audio.version,0)>=1
    if not sound.supported then
        sdk.log("Skyline audio unavailable: install the native Audio API update and rebuild the game.")
        sdk.ui.text("skyline_audio","Sound requires native Audio API extension 1. Physics still works.")
        return
    end
    if sound.prepared then return end
    for _,rpm in ipairs(sound_anchors) do
        sdk.audio.preload("audio/engine_"..rpm.."_coast.wav")
        sdk.audio.preload("audio/engine_"..rpm.."_load.wav")
    end
    sdk.audio.preload("audio/turbo_loop.wav")
    sdk.audio.preload("audio/lift.wav")
    for i=1,6 do sdk.audio.preload(string.format("audio/pop_%02d.wav",i)) end
    sound.prepared=true
    sdk.ui.text("skyline_audio","")
end
local function start_audio()
    if not sound.supported or sdk.settings.audio_enabled==false or sound.running then return end
    sound.clock=0; sound.cooldown=0; sound.rpm=math.max(state.rpm,200)
    sound.throttle=0; sound.previous_throttle=0; sound.overrun_remaining=0
    sound.chop_until=0
    for _,rpm in ipairs(sound_anchors) do
        for _,layer in ipairs({"coast","load"}) do
            sdk.audio.play("engine_"..rpm.."_"..layer,{
                path="audio/engine_"..rpm.."_"..layer..".wav",
                body=BODY,offset={0,-0.1,0.45},loop=true,volume=0,
                pitch=clamp(sound.rpm/rpm,0.25,4),spatial=true,
                spatial_scale=0.10,fade_in=0.08,
            })
        end
    end
    sdk.audio.play("turbo",{
        path="audio/turbo_loop.wav",body=BODY,offset={0,0,0.8},
        loop=true,volume=0,pitch=1,spatial=true,spatial_scale=0.1,fade_in=0.08,
    })
    sound.running=true
end
local function exhaust_pop(intensity)
    if not sound.running then return end
    local master=clamp(number(sdk.settings.engine_volume,0.70),0,1)
    local volume=clamp(number(sdk.settings.pop_volume,0.65),0,1)
    if master*volume<=0 then return end
    sound.pop_slot=sound.pop_slot%6+1
    local variant=1+math.floor(sound_random()*6)
    sdk.audio.play("exhaust_pop_"..sound.pop_slot,{
        path=string.format("audio/pop_%02d.wav",variant),
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
    -- Native host pauses sinks in menus and replay. This also protects test hosts.
    if sdk.snapshot.paused or sdk.snapshot.replay then return end
    local dt=clamp(number(event and event.dt,1/60),0,0.10)
    sound.clock=sound.clock+dt
    local actual_rpm=clamp(number(state.rpm,0),0,12000)
    sound.rpm=sound.rpm+(actual_rpm-sound.rpm)*(1-math.exp(-dt/0.04))
    local throttle=clamp(number(sound.throttle,0),0,1)
    local master=clamp(number(sdk.settings.engine_volume,0.70),0,1)
    sound.cooldown=math.max(0,sound.cooldown-dt)
    -- Existing physics uses a SOFT fuel taper. Detect its near-redline region;
    -- this acoustic chatter does not impose a new RPM/clutch/ignition model.
    local limiter=actual_rpm>=C.redline_rpm-160 and throttle>0.55 and sound.shift<=0
    if limiter and sdk.settings.redline_pops~=false then
        sound.overrun_remaining=0
        if sound.cooldown<=0 then
            exhaust_pop(0.78+0.20*sound_random())
            sound.chop_until=sound.clock+0.022+0.012*sound_random()
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
                    volume=master*0.30,pitch=0.9+0.2*sound_random(),
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
    local rpm=math.max(sound.rpm,200)
    local low=1
    for i=1,#sound_anchors-1 do if rpm>=sound_anchors[i] then low=i end end
    if rpm>=sound_anchors[#sound_anchors] then low=#sound_anchors end
    local high=math.min(low+1,#sound_anchors)
    local mix=0
    if low~=high then
        mix=clamp(math.log(rpm/sound_anchors[low])/math.log(sound_anchors[high]/sound_anchors[low]),0,1)
    end
    local load=throttle^0.70
    local alive=clamp((actual_rpm-150)/450,0,1)
    local chop=sound.clock<sound.chop_until and 0.22 or 1.0
    local shifting=sound.shift>0 and 0.52 or 1.0
    local gain=master*0.50*alive*(0.65+0.35*load)*chop*shifting
    for i,anchor in ipairs(sound_anchors) do
        local weight=0
        if i==low then weight=low==high and 1 or math.sqrt(1-mix)
        elseif i==high then weight=math.sqrt(mix) end
        local pitch=clamp(rpm/anchor,0.25,4)
        sdk.audio.update("engine_"..anchor.."_coast",{
            volume=gain*weight*math.sqrt(1-load),pitch=pitch,
        })
        sdk.audio.update("engine_"..anchor.."_load",{
            volume=gain*weight*math.sqrt(load),pitch=pitch,
        })
    end
    local boost=clamp((rpm-2300)/3700,0,1)*load
    sdk.audio.update("turbo",{
        volume=sdk.settings.turbo_audio==false and 0 or master*0.16*boost,
        pitch=clamp(0.65+rpm/9000,0.25,4),
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
    text("hint",16,161,312,16,"X / C  CAMERA       H  DEBUG",10,muted)
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
    stop_audio(); clear_presentation()
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
    state.spawned=false; state.occupied=false
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
        shape={type="box",half_extents=C.hull_half},
        body_type="dynamic",mass=C.mass,position={position[1],height,position[3]},
        heading=heading,friction=0.25,ccd=true,
        center_of_mass=C.center,inertia_half_extents=C.inertia_half,
        linear_damping=0,angular_damping=0,
    })
    sdk.graphics.mesh("skyline_visual",{path=MODEL,body=BODY})
    reset_simulation()
    state.spawned=true; state.enter_after=state.time+0.25
    start_audio()
    if request.reenter then
        sdk.player.attach(BODY,{-0.38,0.12,-0.23})
        state.occupied=true; configure_camera()
    end
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
                local c={wheel=w,wheel_index=i,point=point,alignment=alignment,
                    length=length,compression=w.rest_length-length,
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
    for _,c in ipairs(contacts) do
        c.wheel.load=c.pn/dt
        local vx,vy=velocity_along(sim,c.x),velocity_along(sim,c.y)
        local tread=c.wheel.omega*C.radius
        c.wheel.slip=tread-vx
        c.wheel.kappa=(tread-vx)/math.max(math.abs(vx),math.abs(tread),0.5)
        c.wheel.alpha=math.deg(math.atan(vy,math.max(math.abs(vx),0.1)))
        c.wheel.fx=c.px/dt; c.wheel.fy=c.py/dt
        c.wheel.usage=c.limit and math.sqrt(c.px*c.px+c.py*c.py)/math.max(c.limit,1e-8) or 0
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
    sound.throttle=input.throttle; sound.shift=state.shift
    local body_velocity=unrotate(car.rotation,car.linvel)
    presentation.speed=math.sqrt(body_velocity[1]^2+body_velocity[3]^2)
    presentation.handbrake=input.hand
    publish_hud(car,input,contacts,dt)
end

return {
    on_load=function()
        reset_simulation(); remove_rig(); prepare_audio(); prepare_presentation()
        sdk.log("Skyline DRIVE: dynamic chase/hood, clean dashboard; physics preserved except explicit aero and optional input shaping")
        sdk.ui.text("skyline_status",""); sdk.ui.text("skyline_handling",""); sdk.ui.text("skyline_wheels","")
    end,
    on_update=function(event) update_audio(event); update_dashboard(event) end,
    on_settings=function(event)
        if event.key=="audio_enabled" and event.value==false then stop_audio() end
        if event.key=="show_driving_debug" then set_debug_visible(event.value==true) end
        if event.key=="show_speedometer" and event.value==false and presentation.supported then
            sdk.ui.remove("driving_dashboard");presentation.active_hud=false
        end
        presentation.hud_timer=0
        if event.key:match("^camera_") or event.key=="hood_height" then configure_camera() end
    end,
    on_unload=function()
        remove_rig()
        sdk.ui.text("skyline_audio",""); sdk.ui.text("skyline_presentation","")
        sdk.ui.text("skyline_help",""); sdk.ui.text("skyline_status",""); sdk.ui.text("skyline_handling",""); sdk.ui.text("skyline_wheels","")
    end,
    on_fixed_update=function(event)
        local dt=number(event and event.dt,1/120)
        if dt<=0 or dt>0.1 then return end
        state.time=state.time+dt
        local p=sdk.input.pad(); local buttons=math.floor(number(p.buttons,0))
        -- Sample edges unconditionally; held keys must not become new presses
        -- merely because another shortcut returned early this tick.
        local spawn=edge("F10")
        local reset_key=edge("KeyR")
        local enter_key=edge("KeyE")
        local camera_key=edge("KeyC")
        local debug_key=edge("KeyH")
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
                if enter then
                    if state.occupied then
                        sdk.player.detach(); state.occupied=false; clear_presentation()
                        if presentation.debug then set_debug_visible(true) end
                    elseif state.time>=state.enter_after then
                        local player=sdk.player.read()
                        if vec(player.position) and dot(sub(car.position,player.position),sub(car.position,player.position))<64 then
                            sdk.player.attach(BODY,{-0.38,0.12,-0.23})
                            state.occupied=true; configure_camera()
                        end
                    end
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
            stop_audio(); clear_presentation(); sound.prepared=false; prepare_audio()
            state.spawned=false; state.occupied=false; state.pending=nil
            state.keys={}; state.buttons=0; reset_simulation()
            sdk.ui.text("skyline_status",""); sdk.ui.text("skyline_handling",""); sdk.ui.text("skyline_wheels","")
        end
    end,
}
