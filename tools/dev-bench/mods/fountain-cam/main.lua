-- Dev only: Better Water on and a fixed camera for --verify captures (written per run by the capture commands).
local SPOT = { player = { 352.0, 68.5, -330.0 }, eye = { 341.5, 70.6, -289.0 }, look = { 341.5, 67.9, -305.0 } }
local LOOK = { enabled = true }
local frames = 0
return {
  on_update = function(event)
    frames = frames + 1
    if frames == 120 then
      sdk.graphics.water_look(LOOK)
      sdk.player.teleport({ position = SPOT.player, heading = 0, velocity = { 0, 0, 0 } })
    end
    if frames >= 120 then sdk.camera.set(SPOT.eye, SPOT.look) end
  end,
}
