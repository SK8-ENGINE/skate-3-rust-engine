 N-- Propulsion is exclusively axle-joint motors plus Rapier tire friction.
-- Steering is exclusively front revolute motors. Suspension is four
-- prismatic joints with physical springs. No drive/steering forces or impulses.
local MODEL = "skyline.glb"
local CHASSIS = "chassis"
local SEAT = {-0.38, 0.12, -0.23}
local CAMERA = {0, 2.1, -5.8}
local ENTER_RANGE = 8.0
local WHEEL_RADIUS = 0.34
local WHEEL_HALF_WIDTH = 0.146
local SUSPENSION_TRAVEL = 0.30
local SUSPENSION_STIFFNESS = 26000
local SUSPENSION_DAMPING = 5600
local STEER_LIMIT = 0.65
local WHEELBASE = 2.7906
local TRACK_WIDTH = 1.591
local FINAL_DRIVE = 3.545
local GEAR_RATIOS = {3.827, 2.360, 1.685, 1.312, 1.000, 0.793}
local GEAR_TORQUE_COMPENSATION = {1.0, 2.0, 4.0, 5.0, 6.0, 7.0}
local REVERSE_RATIO = 3.280
local IDLE_RPM = 900
local REDLINE_RPM = 8000
local SHIFT_TIME = 0.18
local DRIVELINE_EFFICIENCY = 0.85
-- Converts RB26 crank torque to Rapier's constrained axle-motor force scale.
local JOINT_TORQUE_CALIBRATION = 2.5
local TWO_PI = math.pi * 2

local WHEELS = {
  { key = "wheel_rl", local_pos = { 0.795353, -0.330587, -1.347033 }, driven = true, steer = false, drive_share = 0.45 },
  { key = "wheel_rr", local_pos = {-0.796153, -0.330635, -1.347024 }, driven = true, steer = false, drive_share = 0.45 },
  { key = "wheel_fl", local_pos = { 0.794235, -0.330559,  1.443641 }, driven = true, steer = true,  drive_share = 0.05 },
  { key = "wheel_fr", local_pos = {-0.796149, -0.330652,  1.443556 }, driven = true, steer = true,  drive_share = 0.05 },
}

local spawned = false
local occupied = false
local pending_spawn = false
local previous = {}
local previous_buttons = 0
local previous_y = false
local elapsed = 0
local enter_after = 0
local gear = 1
local shift_timer = 0
local pending_pose = nil
local pending_reenter = false
local reset_was_down = false

local function pad()
  return sdk.input.pad() or {
    buttons = 0,
    triggers = {0, 0},
    left = {0, 0},
    right = {0, 0},
  }
end

local function down(key)
  return sdk.input.down(key) == true
end

local function pressed(key)
  local held = down(key)
  local edge = held and not previous[key]
  previous[key] = held
  return edge
end

local function y_pressed()
  local buttons = math.floor(tonumber(pad().buttons) or 0)
  local held = (buttons & 0x8000) ~= 0
  local edge = held and not previous_y
  previous_y = held
  return edge
end

local function clamp(value, low, high)
  if value < low then return low end
  if value > high then return high end
  return value
end

local function wheel_collider()
  local points = {}
  for index = 0, 63 do
    local angle = TWO_PI * index / 64
    local y = math.cos(angle) * WHEEL_RADIUS
    local z = math.sin(angle) * WHEEL_RADIUS
    points[#points + 1] = {-WHEEL_HALF_WIDTH, y, z}
    points[#points + 1] = { WHEEL_HALF_WIDTH, y, z}
  end
  return {type = "convex", points = points}
end

local function quat_yaw(q)
  return math.atan(2 * (q[4] * q[2] + q[1] * q[3]),
    1 - 2 * (q[2] * q[2] + q[3] * q[3]))
end

local function gear_ratio()
  if gear == -1 then return REVERSE_RATIO * FINAL_DRIVE end
  if gear == 0 then return nil end
  return GEAR_RATIOS[gear] * FINAL_DRIVE
end

local function gear_label()
  if gear == -1 then return "R" end
  if gear == 0 then return "N" end
  return tostring(gear)
end

local function engine_torque(rpm)
  local curve = {
    {900, 210},
    {2000, 300},
    {3000, 350},
    {4400, 392},
    {6800, 380},
    {8000, 300},
  }
  rpm = clamp(rpm, curve[1][1], curve[#curve][1])
  for index = 2, #curve do
    local high = curve[index]
    if rpm <= high[1] then
      local low = curve[index - 1]
      local t = (rpm - low[1]) / (high[1] - low[1])
      return low[2] + (high[2] - low[2]) * t
    end
  end
  return curve[#curve][2]
end

local function yaw_quat(heading)
  local half = heading * 0.5
  return {0, math.sin(half), 0, math.cos(half)}
end

local function rotate(q, v)
  local x, y, z, w = q[1], q[2], q[3], q[4]
  local ix = w * v[1] + y * v[3] - z * v[2]
  local iy = w * v[2] + z * v[1] - x * v[3]
  local iz = w * v[3] + x * v[2] - y * v[1]
  local iw = -x * v[1] - y * v[2] - z * v[3]
  return {
    ix * w + iw * -x + iy * -z - iz * -y,
    iy * w + iw * -y + iz * -x - ix * -z,
    iz * w + iw * -z + ix * -y - iy * -x,
  }
end

local function relative_yaw(a, b)
  local ax, ay, az, aw = -a[1], -a[2], -a[3], a[4]
  local bx, by, bz, bw = b[1], b[2], b[3], b[4]
  local x = aw * bx + ax * bw + ay * bz - az * by
  local y = aw * by - ax * bz + ay * bw + az * bx
  local z = aw * bz + ax * by - ay * bx + az * bw
  local w = aw * bw - ax * bx - ay * by - az * bz
  return math.atan(2 * (w * y + x * z), 1 - 2 * (y * y + z * z))
end

local function add(a, b)
  return {a[1] + b[1], a[2] + b[2], a[3] + b[3]}
end

local function terrain_y(x, z)
  local hit = sdk.physics.raycast(
    {x, 500, z},
    {0, -1, 0},
    {max_distance = 1000, filter = "ground"}
  )
  return hit and hit.point and hit.point[2] or nil
end

local function carrier_key(wheel)
  return "carrier_" .. wheel.key
end

local function knuckle_key(wheel)
  return "knuckle_" .. wheel.key
end

local function suspension_key(wheel)
  return "suspension_" .. wheel.key
end

local function axle_key(wheel)
  return "axle_" .. wheel.key
end

local function steering_key(wheel)
  return "steering_" .. wheel.key
end

local function wheel_contact(key)
  for _, pair in ipairs(sdk.physics.touching() or {}) do
    if (pair.a == key and pair.b == "ground")
      or (pair.b == key and pair.a == "ground") then
      return true
    end
  end
  return false
end

local function stop_motor(key)
  sdk.physics.joint_motor(key, {
    mode = "velocity",
    target_velocity = 0,
    factor = 0,
    max_force = 0,
  })
end

local function steer_to(wheel, target)
  local carrier = sdk.physics.read(carrier_key(wheel))
  local knuckle = sdk.physics.read(knuckle_key(wheel))
  local current = 0
  if carrier and carrier.rotation and knuckle and knuckle.rotation then
    current = relative_yaw(carrier.rotation, knuckle.rotation)
  end
  sdk.physics.joint_motor(steering_key(wheel), {
    mode = "velocity",
    target_velocity = clamp((target - current) * 10, -5, 5),
    factor = 5000,
    max_force = 30000,
  })
end

local function steering_targets(input, speed)
  local angle = -input * STEER_LIMIT / (1 + speed * speed * 0.0015)
  if math.abs(angle) < 0.001 then
    return {wheel_fl = 0, wheel_fr = 0}
  end
  local sign = angle < 0 and -1 or 1
  local radius = WHEELBASE / math.tan(math.abs(angle))
  local inner = math.atan(WHEELBASE / math.max(0.2, radius - TRACK_WIDTH * 0.5))
  local outer = math.atan(WHEELBASE / (radius + TRACK_WIDTH * 0.5))
  if sign < 0 then
    return {wheel_fl = -outer, wheel_fr = -inner}
  end
  return {wheel_fl = inner, wheel_fr = outer}
end

local function wheel_omega(wheel)
  local body = sdk.physics.read(wheel.key)
  if not body or not body.rotation or not body.angvel then return 0 end
  local axle = rotate(body.rotation, {1, 0, 0})
  return body.angvel[1] * axle[1]
    + body.angvel[2] * axle[2]
    + body.angvel[3] * axle[3]
end

local function driven_wheel_omega()
  local total, count = 0, 0
  for _, wheel in ipairs(WHEELS) do
    if wheel.driven then
      total = total + math.abs(wheel_omega(wheel))
      count = count + 1
    end
  end
  return count > 0 and total / count or 0
end

local function remove_rig()
  sdk.player.detach()
  sdk.camera.clear_follow()
  for _, wheel in ipairs(WHEELS) do
    sdk.physics.remove_joint(axle_key(wheel))
    sdk.physics.remove_joint(suspension_key(wheel))
    if wheel.steer then
      sdk.physics.remove_joint(steering_key(wheel))
      sdk.physics.remove(knuckle_key(wheel))
    end
    sdk.physics.remove(wheel.key)
    sdk.physics.remove(carrier_key(wheel))
  end
  sdk.physics.remove(CHASSIS)
  sdk.graphics.remove("skyline_visual")
  spawned = false
  occupied = false
end

local function spawn_rig()
  local player = sdk.player.read()
  local pose = pending_pose
  pending_pose = nil
  local heading = pose and pose.heading or player.heading or 0
  local rotation = yaw_quat(heading)
  local forward = rotate(rotation, {0, 0, 1})
  local x = pose and pose.x or player.position[1] + forward[1] * 4
  local z = pose and pose.z or player.position[3] + forward[3] * 4
  local chassis_y = -math.huge

  for _, wheel in ipairs(WHEELS) do
    local offset = rotate(rotation, wheel.local_pos)
    local ground = terrain_y(x + offset[1], z + offset[3])
    if not ground then
      sdk.log("skyline: no Rapier terrain below wheel " .. wheel.key)
      return false
    end
    chassis_y = math.max(chassis_y, ground + WHEEL_RADIUS - offset[2])
  end

  local chassis_position = {x, chassis_y, z}
  sdk.physics.spawn(CHASSIS, {
    shape = {type = "mesh", path = MODEL, object = "skyline_mesh"},
    body_type = "dynamic",
    mass = 1400,
    position = chassis_position,
    heading = heading,
    friction = 0.45,
    membership = 1,
    filter = 1,
    ccd = true,
    center_of_mass = {0, -0.35, -0.18},
    inertia_half_extents = {1.02, 0.61, 2.43},
    linear_damping = 0.025,
    angular_damping = 0.12,
  })

  local tire_friction = sdk.settings.tire_friction or 1.3
  for _, wheel in ipairs(WHEELS) do
    local center = add(chassis_position, rotate(rotation, wheel.local_pos))
    local carrier = carrier_key(wheel)
    sdk.physics.spawn(carrier, {
      shape = {type = "sphere", radius = 0.055},
      body_type = "dynamic",
      mass = 30,
      position = center,
      heading = heading,
      friction = 0,
      sensor = true,
      ccd = true,
      inertia_half_extents = {0.18, 0.18, 0.18},
      linear_damping = 0.05,
      angular_damping = 0.2,
    })
    sdk.physics.spawn(wheel.key, {
      -- Smooth, GLB-dimensioned tire proxy. The authored tread is visually
      -- correct but produces polygon impacts when used as a rolling collider.
      shape = wheel_collider(),
      body_type = "dynamic",
      mass = 28,
      position = center,
      heading = heading,
      friction = tire_friction,
      membership = 2,
      filter = 2,
      ccd = true,
      linear_damping = 0.01,
      angular_damping = 0.015,
    })
    sdk.physics.prismatic(suspension_key(wheel), {
      body_a = CHASSIS,
      body_b = carrier,
      anchor_a = wheel.local_pos,
      anchor_b = {0, 0, 0},
      axis = {0, -1, 0},
      limits = {-SUSPENSION_TRAVEL, SUSPENSION_TRAVEL},
      contacts_enabled = false,
    })
    sdk.physics.joint_spring(suspension_key(wheel), {
      position = 0,
      stiffness = SUSPENSION_STIFFNESS,
      damping = SUSPENSION_DAMPING,
      max_force = 18000,
    })

    local axle_parent = carrier
    if wheel.steer then
      local knuckle = knuckle_key(wheel)
      sdk.physics.spawn(knuckle, {
        shape = {type = "sphere", radius = 0.05},
        body_type = "dynamic",
        mass = 24,
        position = center,
        heading = heading,
        friction = 0,
        sensor = true,
        ccd = true,
        inertia_half_extents = {0.20, 0.22, 0.20},
        linear_damping = 0.05,
        angular_damping = 0.2,
      })
      sdk.physics.revolute(steering_key(wheel), {
        body_a = carrier,
        body_b = knuckle,
        anchor_a = {0, 0, 0},
        anchor_b = {0, 0, 0},
        axis = {0, 1, 0},
        limits = {-STEER_LIMIT, STEER_LIMIT},
        contacts_enabled = false,
      })
      sdk.physics.joint_motor(steering_key(wheel), {
        mode = "velocity",
        target_velocity = 0,
        factor = 5000,
        max_force = 30000,
      })
      axle_parent = knuckle
    end

    sdk.physics.revolute(axle_key(wheel), {
      body_a = axle_parent,
      body_b = wheel.key,
      anchor_a = {0, 0, 0},
      anchor_b = {0, 0, 0},
      axis = {1, 0, 0},
      contacts_enabled = false,
    })
    stop_motor(axle_key(wheel))
  end

  sdk.graphics.mesh("skyline_visual", {path = MODEL, body = CHASSIS})
  spawned = true
  gear = 1
  shift_timer = 0
  previous_buttons = 0
  previous_y = false
  reset_was_down = false
  enter_after = elapsed + 0.5
  if pending_reenter then
    sdk.player.attach(CHASSIS, SEAT)
    sdk.camera.follow(CHASSIS, CAMERA)
    occupied = true
  end
  pending_reenter = false
  sdk.log("skyline: spawned physical 11-body/10-joint assembly")
  return true
end

local function request_respawn()
  if spawned then
    remove_rig()
  end
  pending_pose = nil
  pending_reenter = false
  pending_spawn = true
end

local function request_reset(car)
  pending_pose = {
    x = car.position[1],
    z = car.position[3],
    heading = quat_yaw(car.rotation),
  }
  pending_reenter = occupied
  remove_rig()
  pending_spawn = true
end

local function drive(car, dt)
  local controller = pad()
  local triggers = controller.triggers or {0, 0}
  local throttle = clamp(
    (tonumber(triggers[2]) or 0) + (down("KeyW") and 1 or 0),
    0,
    1
  )
  local steer = clamp(
    ((controller.left or {0, 0})[1] or 0)
      + (down("KeyD") and 1 or 0) - (down("KeyA") and 1 or 0),
    -1,
    1
  )
  local buttons = math.floor(tonumber(controller.buttons) or 0)
  local rb = (buttons & 0x0200) ~= 0
  local lb = (buttons & 0x0100) ~= 0
  local old_rb = (previous_buttons & 0x0200) ~= 0
  local old_lb = (previous_buttons & 0x0100) ~= 0
  if rb and not old_rb and gear < #GEAR_RATIOS then
    gear = gear + 1
    shift_timer = SHIFT_TIME
  elseif lb and not old_lb and gear > -1 then
    gear = gear - 1
    shift_timer = SHIFT_TIME
  end
  previous_buttons = buttons
  shift_timer = math.max(0, shift_timer - dt)

  local brake_amount = clamp(
    (tonumber(triggers[1]) or 0) + (down("KeyS") and 1 or 0),
    0,
    1
  )
  local foot_braking = brake_amount > 0.01 or down("Space")
    or ((buttons & 0x1000) ~= 0)
  local handbrake = down("ShiftLeft") or ((buttons & 0x2000) ~= 0)
  local ratio = gear_ratio()
  local wheel_rpm = driven_wheel_omega() * 60 / TWO_PI
  local raw_rpm = ratio and wheel_rpm * ratio or 0
  local rpm = ratio and clamp(raw_rpm, IDLE_RPM, REDLINE_RPM) or IDLE_RPM
  -- Keep the velocity servo out of the torque curve. Engine torque is capped
  -- separately at redline; targeting redline directly would taper motor force
  -- thousands of RPM early as velocity error shrinks.
  local target_omega = ratio and REDLINE_RPM * 1.5 * TWO_PI / 60 / ratio or 0
  if gear == -1 then target_omega = -target_omega end
  local on_limiter = raw_rpm >= REDLINE_RPM
  local total_axle_torque = ratio
    and engine_torque(rpm) * ratio * DRIVELINE_EFFICIENCY * JOINT_TORQUE_CALIBRATION
      * (gear > 0 and GEAR_TORQUE_COMPENSATION[gear] or 1)
    or 0
  local steering = steering_targets(steer, car.speed or 0)

  for _, wheel in ipairs(WHEELS) do
    if wheel.steer then
      steer_to(wheel, steering[wheel.key])
    end
    if wheel.driven then
      if foot_braking or (handbrake and not wheel.steer) then
        sdk.physics.joint_motor(axle_key(wheel), {
          mode = "velocity",
          target_velocity = 0,
          factor = 120,
          max_force = handbrake and not wheel.steer and 12000 or 9000,
        })
      elseif handbrake then
        stop_motor(axle_key(wheel))
      elseif throttle > 0.01 and shift_timer <= 0 and ratio and not on_limiter then
        -- ATTESA closes toward even AWD for a straight launch, then releases
        -- the front axle during cornering so throttle can rotate the rear.
        local drive_share = 0.25
        if math.abs(steer) > 0.15 then
          drive_share = wheel.steer and 0.05 or 0.45
        end
        sdk.physics.joint_motor(axle_key(wheel), {
          mode = "velocity",
          target_velocity = target_omega,
          factor = 10000,
          max_force = total_axle_torque * drive_share * throttle,
        })
      else
        stop_motor(axle_key(wheel))
      end
    end
  end

  if sdk.settings.show_debug ~= false then
    local contacts = 0
    for _, wheel in ipairs(WHEELS) do
      if wheel_contact(wheel.key) then contacts = contacts + 1 end
    end
    sdk.ui.text("skyline_status", string.format(
      "Skyline physics | %.0f km/h | gear %s | %.0f rpm | tires %d/4 | throttle %.0f%% | steer %.0f%%",
      (car.speed or 0) * 3.6,
      gear_label(),
      rpm,
      contacts,
      throttle * 100,
      steer * 100
    ))
  end
end

return {
  on_load = function()
    sdk.ui.text("skyline_help", "Skyline — F10 spawn · Y/E enter · RT throttle · LT/A brake · B/Shift handbrake · RB/LB shift · R/R3 reset")
  end,

  on_unload = function()
    remove_rig()
  end,

  on_fixed_update = function(event)
    elapsed = elapsed + (event and event.dt or 1 / 120)

    if pressed("F10") then
      request_respawn()
      return
    end
    if pending_spawn then
      pending_spawn = false
      spawn_rig()
      return
    end

    local car = sdk.physics.read(CHASSIS)
    if not car then return end

    local buttons = math.floor(tonumber(pad().buttons) or 0)
    local reset_down = (buttons & 0x0080) ~= 0
    local reset_pressed = reset_down and not reset_was_down
    reset_was_down = reset_down
    if pressed("KeyR") or reset_pressed then
      request_reset(car)
      return
    end

    local enter_pressed = pressed("KeyE")
    local controller_enter = y_pressed()
    if enter_pressed or controller_enter then
      if occupied then
        sdk.player.detach()
        sdk.camera.clear_follow()
        occupied = false
      elseif elapsed >= enter_after then
        local player = sdk.player.read()
        local dx = car.position[1] - player.position[1]
        local dz = car.position[3] - player.position[3]
        if dx * dx + dz * dz <= ENTER_RANGE * ENTER_RANGE then
          sdk.player.attach(CHASSIS, SEAT)
          sdk.camera.follow(CHASSIS, CAMERA)
          occupied = true
        end
      end
    end

    if occupied then
      drive(car, event and event.dt or 1 / 120)
    else
      for _, wheel in ipairs(WHEELS) do
        if wheel.driven then stop_motor(axle_key(wheel)) end
        if wheel.steer then
          steer_to(wheel, 0)
        end
      end
    end
  end,

  on_event = function(event)
    if event.name == "world_changed" then
      spawned = false
      occupied = false
      pending_spawn = false
      previous = {}
      previous_buttons = 0
      previous_y = false
      gear = 1
      shift_timer = 0
      pending_pose = nil
      pending_reenter = false
      reset_was_down = false
    end
  end,
}
