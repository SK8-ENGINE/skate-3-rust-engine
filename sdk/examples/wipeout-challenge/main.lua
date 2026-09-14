local WAVE_LEN = 40
local MAXZ = 24
local SKY_LIFT = 40
local FALL_MARGIN = 12

local RED = { 1, 0.12, 0.12 }
local WATER = { 0.1, 0.55, 1 }
local SPIKE = { 1, 0.5, 0.05 }
local DECK = { 0.55, 0.6, 0.7 }
local STARTPAD = { 0.15, 0.9, 0.35 }

local run = {
  started = false, over = false,
  bail_used = false, hits = 0,
  t0 = 0,
  ax = 0, ay = 0, az = 0,
  built = false, grace_until = 0,
}
local prev = { r = false, t = false }
local slots = {}
for i = 1, MAXZ do slots[i] = { live = false, x = 0, y = 0, z = 0, hx = 0, hy = 0, hz = 0, kind = 0 } end
local scratch = {}

local function fmt_time(s)
  if s < 0 then s = 0 end
  local m = math.floor(s / 60)
  local sec = math.floor(s % 60)
  local ms = math.floor((s * 1000) % 1000)
  return string.format("%d:%02d.%03d", m, sec, ms)
end

local function setting(key, fallback)
  local v = sdk.settings[key]
  if v == nil then return fallback end
  return v
end

local function intensity_scale()
  local h = setting("hazard_intensity", "hardcore")
  if h == "easy" then return 0.5 end
  if h == "normal" then return 0.75 end
  return 1
end

local function wave_count()
  local w = math.floor(setting("wave_count", 4))
  if w < 1 then w = 1 end
  if w > 10 then w = 10 end
  return w
end

local function prand(seed)
  local x = math.sin(seed * 127.1) * 43758.5
  return x - math.floor(x)
end

local function slot_key(i)
  return string.format("wz%02d", i)
end

local function zone_color(kind)
  if kind == 2 then return WATER end
  if kind == 3 then return SPIKE end
  return RED
end

-- Course pieces, anchor-relative. Water keeps its original surface height
-- with depth below it so a falling skater cannot skip a thin sampled sensor.
-- kind 9 = deck platform (no hit rules).
local function course_pieces()
  return {
    { key = "wo.p1", x = 0, y = 0, z = 0, hx = 7, hy = 0.5, hz = 7, kind = 9, color = STARTPAD },
    { key = "wo.p2", x = 17, y = 1, z = 0, hx = 5, hy = 0.5, hz = 5, kind = 9, color = DECK },
    { key = "wo.p3", x = 17, y = 2, z = 17, hx = 5, hy = 0.5, hz = 5, kind = 9, color = DECK },
    { key = "wo.p4", x = 0, y = 3, z = 17, hx = 7, hy = 0.5, hz = 7, kind = 9, color = DECK },
    { key = "wo.w1", x = 8.5, y = -1.22, z = 0, hx = 2.5, hy = 2.0, hz = 6, kind = 2, sensor = true },
    { key = "wo.w2", x = 17, y = -0.22, z = 8.5, hx = 4, hy = 2.0, hz = 2.5, kind = 2, sensor = true },
    { key = "wo.w3", x = 8.5, y = 0.78, z = 17, hx = 2.5, hy = 2.0, hz = 6, kind = 2, sensor = true },
  }
end

local function build_course()
  local ax, ay, az = run.ax, run.ay, run.az
  for _, pc in ipairs(course_pieces()) do
    local desc = {
      shape = { type = "box", half_extents = { pc.hx, pc.hy, pc.hz } },
      body_type = "static",
      position = { ax + pc.x, ay + pc.y, az + pc.z },
      friction = 0.9,
    }
    if pc.sensor then desc.sensor = true end
    sdk.physics.spawn(pc.key, desc)
    sdk.graphics.mesh(pc.key .. "v", {
      body = pc.key,
      scale = { pc.hx * 2, pc.hy * 2, pc.hz * 2 },
      color = pc.color or zone_color(pc.kind),
    })
  end
  run.built = true
end

local function clear_course()
  for _, pc in ipairs(course_pieces()) do
    sdk.physics.remove(pc.key)
    sdk.graphics.remove(pc.key .. "v")
  end
  run.built = false
end

local function start_pad_top()
  -- Pad corner, clear of the wave-1 orbit ring.
  return { run.ax - 5, run.ay + 1.6, run.az - 5 }
end

local function teleport_to_start()
  local s = start_pad_top()
  sdk.player.teleport({position=s, heading=0, velocity={0,0,0}})
end

-- Moving hazards, anchor-relative. kind 1=red 2=water 3=spike.
local function gen_zones(wave, t, out)
  local ax, ay, az = run.ax, run.ay, run.az
  local n = 0
  local scale = intensity_scale()
  local function add(x, y, z, hx, hy, hz, kind)
    n = n + 1
    if n > MAXZ then return end
    out[n] = out[n] or {}
    local zn = out[n]
    zn.x, zn.y, zn.z, zn.hx, zn.hy, zn.hz, zn.kind = x, y, z, hx, hy, hz, kind
  end
  if wave == 1 then
    local count = math.floor(6 * scale + 0.5)
    for i = 1, count do
      local a = (i - 1) * math.pi * 2 / count + t * 0.35
      local r = 4.5 + math.sin(t * 1.5 + i) * 1
      add(ax + math.cos(a) * r, ay + 1.1, az + math.sin(a) * r, 1.0, 0.6, 1.0, 1)
    end
  elseif wave == 2 then
    local rc = math.floor(6 * scale + 0.5)
    for i = 1, rc do
      local a = (i - 1) * math.pi * 2 / rc - t * 0.7
      local r = 3.2 + math.sin(t * 2 + i * 2) * 0.8
      add(ax + 17 + math.cos(a) * r, ay + 2.1, az + math.sin(a) * r, 0.9, 0.6, 0.9, 1)
    end
    local qc = math.floor(4 * scale + 0.5)
    for i = 1, qc do
      local ox = (prand(i * 31 + wave) - 0.5) * 10
      local oz = (prand(i * 57 + wave * 3) - 0.5) * 10
      if math.abs(ox) + math.abs(oz) > 4 then
        add(ax + ox, ay + 1.1, az + oz, 1.0, 0.6, 1.0, 1)
      end
    end
  elseif wave == 3 then
    local sc = math.floor(8 * scale + 0.5)
    for i = 1, sc do
      local a = (i - 1) * math.pi * 2 / sc + t * 1.2
      local r = 3 + math.sin(t * 2.5 + i) * 1
      add(ax + 17 + math.cos(a) * r, ay + 3.4, az + 17 + math.sin(a) * r, 0.85, 0.85, 0.85, 3)
    end
  else
    local cc = math.floor(16 * scale + 0.5)
    local hubs = { { 0, 0.5, 0 }, { 17, 1.5, 0 }, { 17, 2.5, 17 }, { 0, 3.5, 17 } }
    for i = 1, cc do
      local hub = hubs[((i - 1) % 4) + 1]
      local a = (i - 1) * math.pi * 2 / cc + t * (0.6 + (i % 3) * 0.4)
      local r = 3.5 + math.sin(t * 3 + i * 1.7) * 1.5
      local kind = 1
      local h = 0.6
      if i % 5 == 0 then kind = 3 h = 0.85 end
      add(ax + hub[1] + math.cos(a) * r, ay + hub[2] + h, az + hub[3] + math.sin(a) * r,
        1.0, h, 1.0, kind)
    end
  end
  return n
end

local function spawn_zone(i, zn)
  local key = slot_key(i)
  local desc = {
    shape = { type = "box", half_extents = { zn.hx, zn.hy, zn.hz } },
    body_type = "kinematic",
    position = { zn.x, zn.y, zn.z },
    friction = 0.7,
  }
  if zn.kind == 2 then desc.sensor = true end
  sdk.physics.spawn(key, desc)
  sdk.graphics.mesh(key .. "v", {
    body = key,
    scale = { zn.hx * 2, zn.hy * 2, zn.hz * 2 },
    color = zone_color(zn.kind),
  })
  local s = slots[i]
  s.touching = false
  s.live = true
  s.x, s.y, s.z, s.hx, s.hy, s.hz, s.kind = zn.x, zn.y, zn.z, zn.hx, zn.hy, zn.hz, zn.kind
end

local function remove_zone(i)
  local s = slots[i]
  if not s.live then return end
  local key = slot_key(i)
  sdk.physics.remove(key)
  sdk.graphics.remove(key .. "v")
  s.live = false
end


local restore, phase, last_wave = nil, "idle", 0
local function menu()
  sdk.ui.menu("challenge", {title="Wipeout", section="Gamemodes", items={
    {id="start",label="Start course",enabled=phase=="idle"},
    {id="restart",label="Restart at start pad",enabled=phase~="idle"},
    {id="stop",label="Stop and return",enabled=phase~="idle"},
  }})
end
local function clear_slots() for i=1,MAXZ do remove_zone(i) end end
local function stop(return_player)
  if return_player and restore then sdk.player.teleport(restore) end
  clear_slots();clear_course();restore=nil;phase="idle";run.started=false
  sdk.ui.text("wo.status", "Wipeout: open Gamemodes > Wipeout to start.")
  menu()
end
local function begin()
  if not sdk.capabilities.player_overlap then
    sdk.ui.text("wo.status","Wipeout needs the updated player-overlap API. Update the game build.");return
  end
  if not restore then
    local p=sdk.player.read();restore={position=p.position,heading=p.heading,velocity={0,0,0}}
    run.ax,run.ay,run.az=p.position[1],p.position[2]+SKY_LIFT,p.position[3]
    build_course()
  end
  clear_slots();last_wave=0
  run.started,run.over,run.bail_used,run.hits=true,false,false,0
  run.t0=sdk.time.elapsed+2;run.grace_until=run.t0;run.hit_until=run.t0
  phase="running";teleport_to_start();menu()
end
local function finish(won,elapsed)
  phase=won and "cleared" or "eliminated";run.over=true
  clear_slots()
  sdk.ui.text("wo.status",string.upper(phase).." at "..fmt_time(elapsed)..". Restart or Stop in Gamemodes > Wipeout.")
  menu()
end
local function overlapping(key)
  local body=sdk.physics.read(key)
  return body and body.player_overlapping or false
end
local function fixed(ev)
  local t,r=sdk.input.down("KeyT"),sdk.input.down("KeyR")
  if (t and not prev.t and phase=="idle") or (r and not prev.r) then begin() end
  prev.t,prev.r=t,r
  if phase~="running" then return end
  local now=sdk.time.elapsed
  local elapsed=math.max(0,now-run.t0)
  local wave=math.floor(elapsed/WAVE_LEN)+1
  local total=wave_count()
  if setting("game_mode","survival")=="timed" and wave>total then finish(true,elapsed);return end
  wave=(wave-1)%total+1
  if wave~=last_wave then clear_slots();last_wave=wave end
  local dt=math.max(0.001,ev.dt or 1/60)
  local count=gen_zones(wave,elapsed+dt,scratch)
  for i=1,MAXZ do
    if i<=count then
      local z,s=scratch[i],slots[i]
      if not s.live then spawn_zone(i,z)
      else
        local b=sdk.physics.read(slot_key(i))
        if b then sdk.physics.set_linvel(slot_key(i),{(z.x-b.position[1])/dt,(z.y-b.position[2])/dt,(z.z-b.position[3])/dt}) end
      end
    else remove_zone(i) end
  end
  if now>=run.grace_until then
    for _,pc in ipairs(course_pieces()) do
      if pc.sensor and overlapping(pc.key) then finish(false,elapsed);return end
    end
    local p=sdk.player.read()
    if p.position[2]<run.ay-FALL_MARGIN then finish(false,elapsed);return end
    -- Count entries, not every frame spent touching one hazard. One-second
    -- recovery also groups simultaneous impacts from the same collision.
    for i=1,count do
      local s=slots[i];local hit=overlapping(slot_key(i))
      if hit and not s.touching and now>=run.hit_until then
        run.hits=run.hits+1;run.hit_until=now+1
        if run.bail_used then finish(false,elapsed);return end
        run.bail_used=true
      end
      s.touching=hit
    end
  end
  local timer=setting("enable_timer",true) and (" | "..fmt_time(elapsed)) or ""
  sdk.ui.text("wo.status","WIPEOUT | Wave "..wave.."/"..total..timer.." | "..
    (now<run.grace_until and "Get ready" or run.bail_used and "Next hazard hit ends the run" or "One hazard hit forgiven"))
end
return {
  on_load=function() menu();sdk.ui.text("wo.status","Wipeout: open Gamemodes > Wipeout to start.") end,
  on_fixed_update=fixed,
  on_event=function(e)
    if e.name=="world_changed" then stop(false)
    elseif e.name=="menu_action" and e.menu=="challenge" then
      if e.item=="start" and phase=="idle" or e.item=="restart" then begin()
      elseif e.item=="stop" then stop(true) end
    end
  end,
  on_unload=function() stop(true);sdk.ui.remove_menu("challenge");sdk.ui.text("wo.status","") end,
}
