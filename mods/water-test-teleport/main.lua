-- Dev only (branch gameplay/water). F2/F3/F4 drop the skater 2 m above water
-- collision (centroids from `WATER_POINTS=1 cargo run -p skate-data --example
-- water_surfaces`). Shift+F2/F3/F4 stand on dry ground facing that water
-- (`WATER_VIEW=x,y,z[,reach,rise]`). The University spots only make sense on
-- University, the DownTown ones (F4, F7) only on DownTown. F6 is the game's camera key.
local SPOTS = {
  F2 = {
    water = { label = "University fountain basin (water 67.94 m)", position = { 340.3, 69.94, -294.3 }, heading = 0 },
    view = { label = "University fountain basin, view", position = { 346.5, 68.53, -293.2 }, heading = -1.751 },
  },
  F3 = {
    water = { label = "University reservoir (water 217.87 m)", position = { -156.3, 219.87, -1090.2 }, heading = 0 },
    view = { label = "University reservoir, view from the bank", position = { -171.8, 227.26, -1079.2 }, heading = 2.188 },
  },
  F4 = {
    water = { label = "DownTown fountain (water 18.72 m)", position = { 44.3, 20.72, 241.5 }, heading = 0 },
    view = { label = "DownTown fountain, view", position = { 44.2, 19.49, 247.9 }, heading = 3.120 },
  },
  F7 = {
    water = { label = "DownTown Aletown canal (water 8.93 m, 1.1 m deep)", position = { -182.3, 10.93, 465.9 }, heading = 0 },
    view = { label = "DownTown Aletown spawn, facing the canal", position = { -183.4, 12.76, 433.2 }, heading = 0.034 },
  },
}
local held = {}
local show_position = false
-- Seconds the "Teleported" line stays on screen.
local MESSAGE_SECONDS = 5
local message_timer = 0

return {
  on_update = function(event)
    local dt = (event and tonumber(event.dt)) or 0
    if message_timer > 0 then
      message_timer = message_timer - dt
      if message_timer <= 0 then sdk.ui.text("water-test", "") end
    end
    -- F5: toggle an on-screen position readout (to report water locations).
    local f5 = sdk.input.down("F5") == true
    if f5 and not held.F5 then
      show_position = not show_position
      if not show_position then sdk.ui.text("water-position", "") end
    end
    held.F5 = f5
    if show_position then
      local p = sdk.player.read().position
      sdk.ui.text("water-position", string.format("Position: %.1f, %.2f, %.1f", p[1], p[2], p[3]))
    end
    local shift = sdk.input.down("ShiftLeft") == true or sdk.input.down("ShiftRight") == true
    for key, spot in pairs(SPOTS) do
      local down = sdk.input.down(key) == true
      if down and not held[key] then
        local target = shift and spot.view or spot.water
        sdk.player.teleport({ position = target.position, heading = target.heading, velocity = { 0, 0, 0 } })
        sdk.ui.text("water-test", "Teleported: " .. target.label)
        message_timer = MESSAGE_SECONDS
      end
      held[key] = down
    end
  end,
}
