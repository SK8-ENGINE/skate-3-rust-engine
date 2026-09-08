-- The original Lua/JSON example uses the user's separately supplied kart model.
local previous={}
local interact_was_down=false
local reset_was_down=false
local function pressed(key)
 local down=sdk.input.down(key);local edge=down and not previous[key];previous[key]=down;return edge
end
local function tune()
 local s=sdk.settings
 sdk.vehicle.tune('kart',{engine_force=s.engine,max_speed=s.speed,brake_impulse=s.brake,steering_angle=s.steering,tire_grip=s.grip})
end
local function nearby()
 local p=sdk.player.read();local h=p.heading or 0
 return {p.position[1]+math.sin(h)*3,p.position[2]+1,p.position[3]+math.cos(h)*3},h
end
local function spawn()
 local car=sdk.vehicle.read('kart')
 if car and car.occupied then sdk.vehicle.reset('kart',{car.position[1],car.position[2]+1,car.position[3]},car.heading);return end
 if car then sdk.vehicle.remove('kart') end
 local p,h=nearby();sdk.vehicle.spawn('kart','vehicle.json',p,h);tune()
end
return {
 on_load=function() sdk.ui.text('kart-hud','MARIO KART | F10: spawn kart | E: enter/exit') end,
 on_fixed_update=function()
  if pressed('F10') then spawn() end
  local car=sdk.vehicle.read('kart')
  if not car then return end
  local interact=sdk.vehicle.input().interact
  local interact_pressed=interact and not interact_was_down;interact_was_down=interact
  if interact_pressed then if car.occupied then sdk.vehicle.exit('kart') else sdk.vehicle.enter('kart') end end
  local reset_mask=sdk.settings.reset_button=='Left stick' and 0x40 or 0x80
  local reset_down=car.occupied and ((sdk.vehicle.input().pad_buttons or 0) & reset_mask)~=0
  local reset_pressed=reset_down and not reset_was_down;reset_was_down=reset_down
  if pressed('KeyR') or reset_pressed then
   local p,h=nearby()
   if car.occupied then p={car.position[1],car.position[2]+1,car.position[3]};h=car.heading end
   sdk.vehicle.reset('kart',p,h)
  end
  if car.phase=='driving' then sdk.vehicle.control('kart',sdk.vehicle.input() and {
   throttle=sdk.vehicle.input().throttle,steering=sdk.vehicle.input().steering,
   brake=sdk.vehicle.input().brake,handbrake=sdk.vehicle.input().handbrake}) end
  if sdk.settings.hud then sdk.ui.text('kart-hud',string.format('MARIO KART | %.0f km/h | %s | E enter/exit | WASD drive | Space brake | Shift handbrake | R / stick click reset',math.abs(car.speed)*3.6,car.ready and car.phase or 'loading')) else sdk.scene.remove('kart-hud') end
 end,
 on_settings=function() if sdk.vehicle.read('kart') then tune() end end,
 on_event=function(e) if e.name=='world_changed' then previous={};interact_was_down=false;reset_was_down=false;sdk.ui.text('kart-hud','MARIO KART | F10: spawn kart in this map') end end,
}
