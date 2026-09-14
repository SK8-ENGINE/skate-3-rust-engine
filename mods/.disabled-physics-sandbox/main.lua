local box_key = "crate"
local mass = 25
local friction = 0.35
local spawned = false
local attached = false
local prev = { e = false, r = false }
local log_cooldown = 0
local peak_speed = 0
local samples = 0

local function breakaway_newtons()
  return friction * mass * 9.81
end

local function push_newtons()
  return sdk.settings.push_newtons or 350
end

local function ground_y(x, z)
  local hit = sdk.physics.raycast(
    { x, 500, z },
    { 0, -1, 0 },
    { max_distance = 1000, filter = "ground" }
  )
  if hit and hit.point then return hit.point[2] end
  return 0
end

local function touching_ground(key)
  for _, t in ipairs(sdk.physics.touching() or {}) do
    if (t.a == key and t.b == "ground") or (t.b == key and t.a == "ground") then
      return true
    end
  end
  return false
end

local function spawn_box()
  if spawned then return end
  local p = sdk.player.read().position
  local gy = ground_y(p[1] + 3, p[3] + 3)
  sdk.physics.spawn(box_key, {
    shape = { type = "box", half_extents = { 0.5, 0.5, 0.5 } },
    body_type = "dynamic",
    mass = mass,
    position = { p[1] + 3, gy + 0.55, p[3] + 3 },
    heading = 0,
    friction = friction,
    ccd = true,
  })
  sdk.graphics.mesh("crate_vis", {
    body = box_key,
    scale = { 1, 1, 1 },
    color = { 0.2, 0.65, 0.95 },
  })
  sdk.camera.follow(box_key, { 0, 3, -7 })
  sdk.log(string.format(
    "physics-sandbox: crate mass=%.0fkg friction=%.2f breakaway≈%.0fN push=%dN",
    mass, friction, breakaway_newtons(), push_newtons()
  ))
  spawned = true
  peak_speed = 0
  samples = 0
end

local function reset_box()
  sdk.physics.remove(box_key)
  sdk.graphics.remove("crate_vis")
  if attached then
    sdk.player.detach()
    attached = false
  end
  spawned = false
  spawn_box()
end

local function cmd_force()
  local force = push_newtons()
  local f = { 0, 0, 0 }
  local keys = {}
  if sdk.input.down("KeyW") then f[3] = f[3] + force; keys[#keys + 1] = "W" end
  if sdk.input.down("KeyS") then f[3] = f[3] - force; keys[#keys + 1] = "S" end
  if sdk.input.down("KeyA") then f[1] = f[1] - force; keys[#keys + 1] = "A" end
  if sdk.input.down("KeyD") then f[1] = f[1] + force; keys[#keys + 1] = "D" end
  return f, table.concat(keys, "")
end

return {
  on_load = function()
    sdk.ui.text("help", "Physics Sandbox — WASD push · E attach/detach · R reset")
    spawn_box()
  end,

  on_fixed_update = function(ev)
    if not spawned then
      spawn_box()
    end

    local f, keys = cmd_force()
    local driving = f[1] ~= 0 or f[3] ~= 0
    if driving then
      sdk.physics.force(box_key, f)
    end

    local e = sdk.input.down("KeyE")
    if e and not prev.e then
      if attached then
        sdk.player.detach()
        attached = false
        sdk.log("detached")
      else
        sdk.player.attach(box_key, { 0, 1.1, 0 })
        attached = true
        sdk.log("attached to crate")
      end
    end
    prev.e = e

    local r = sdk.input.down("KeyR")
    if r and not prev.r then
      reset_box()
    end
    prev.r = r

    local body = sdk.physics.read(box_key)
    if not body then
      return
    end

    samples = samples + 1
    local speed = body.speed or 0
    if speed > peak_speed then
      peak_speed = speed
    end
    local rf = body.force or { 0, 0, 0 }
    local p = body.position
    local v = body.linvel
    local need = breakaway_newtons()
    local stuck = driving and speed < 0.05 and push_newtons() < need

    sdk.ui.text(
      "status",
      string.format(
        "pos %.1f %.1f %.1f | vel %.2f %.2f %.2f | speed %.2f (peak %.2f)\ncmd force %.0f %.0f %.0f | read force %.0f %.0f %.0f | keys %s | μmg≈%.0fN | gnd %s%s",
        p[1], p[2], p[3],
        v[1], v[2], v[3],
        speed, peak_speed,
        f[1], f[2], f[3],
        rf[1], rf[2], rf[3],
        keys ~= "" and keys or "-",
        need,
        touching_ground(box_key) and "yes" or "no",
        stuck and " | STUCK: raise Push force" or ""
      )
    )

    log_cooldown = log_cooldown - (ev and ev.dt or (1 / 60))
    local runaway = (not driving) and speed > 1.5 and speed >= peak_speed * 0.98
    if log_cooldown <= 0 or runaway or stuck then
      log_cooldown = (runaway or stuck) and 0.25 or 1.0
      sdk.log(string.format(
        "crate tick=%d pos=(%.2f,%.2f,%.2f) vel=(%.2f,%.2f,%.2f) speed=%.2f cmdF=(%.0f,%.0f,%.0f) readF=(%.0f,%.0f,%.0f) keys=%s stuck=%s",
        samples,
        p[1], p[2], p[3],
        v[1], v[2], v[3],
        speed,
        f[1], f[2], f[3],
        rf[1], rf[2], rf[3],
        keys ~= "" and keys or "-",
        stuck and "yes" or "no"
      ))
    end
  end,

  on_event = function(ev)
    if ev.name == "world_changed" then
      spawned = false
      attached = false
      prev.e = false
      prev.r = false
      peak_speed = 0
      samples = 0
    end
  end,
}
