-- Dev only: put the player on foot near a DownTown NPC skater spawn spot (laptop 2026-11-08: none spawn within 60 m
-- of the map spawn, so a fade-in could not be seen up close). The spot and the spawn come from the run's
-- NPC_SKATER_SPAWN lines ([-313.1, 40.3, 13.9] and [-313.9, 30.6, 84.5] seen in patch1030 / patch1055 runs).
local SPAWN = { -313.1, 40.3, 13.9 }
local STAND = { -305.0, 40.3, 21.0 }
local DELAY_SECONDS = 3
local elapsed = 0
local done = false
local turned = false
local held = {}

local function heading()
  local dx, dz = SPAWN[1] - STAND[1], SPAWN[3] - STAND[3]
  local h = math.atan(dx, dz)
  if turned then h = h + math.pi end
  return h
end

local function go()
  -- The mod sandbox has no pcall: call the teleport directly.
  sdk.player.teleport({ position = STAND, heading = heading(), on_board = false })
  sdk.log(string.format("dev-npc-watch-spot: teleported to [%.1f, %.1f, %.1f] heading %.3f", STAND[1], STAND[2], STAND[3], heading()))
end

return {
  on_update = function(event)
    local dt = (event and tonumber(event.dt)) or 0
    if not done then
      elapsed = elapsed + dt
      if elapsed >= DELAY_SECONDS then
        done = true
        go()
      end
    end
    local f8 = sdk.input.down("F8") == true
    if f8 and not held.F8 then
      if sdk.input.down("ShiftLeft") == true or sdk.input.down("ShiftRight") == true then turned = not turned end
      go()
    end
    held.F8 = f8
  end,
}
