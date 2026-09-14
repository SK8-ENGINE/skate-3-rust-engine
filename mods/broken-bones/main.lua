-- Injury decisions belong to Lua; the engine exposes generic bodies/constraints.
local broken, pending, last_hit, recent = {}, {}, {}, {}
local last_tick, last_mode, was_enabled, status = nil, nil, nil, ""
local function cfg(k,d) local v=sdk.settings[k];if v==nil then return d end;return v end
local function length(v) v=v or {};return math.sqrt((v[1] or 0)^2+(v[2] or 0)^2+(v[3] or 0)^2) end
local function publish()
  local ids={};for i in pairs(broken) do ids[#ids+1]=i end;table.sort(ids)
  sdk.net.publish("injuries",{v=1,j=ids})
end
local function heal()
  for i in pairs(broken) do sdk.rig.reset_joint(i) end
  for i in pairs(pending) do sdk.rig.reset_joint(i) end
  broken,pending,recent,last_hit={},{},{},{};status="Joints restored";publish()
end
local function menu()
  sdk.ui.menu("bones",{title="Broken bones",section="Skater mods",items={
    {id="heal",label="Heal all injuries",description="Restore this mod's joint and drive overrides."},
  }})
end
local function eligible(j)
  return j and (j.name:find("LEFT",1,true) or j.name:find("RIGHT",1,true))
end
local function draw()
  if not cfg("show_hud",true) then sdk.ui.text("bones","");return end
  local labels={};for _,j in pairs(broken) do labels[#labels+1]=j.name:gsub("JOINT_",""):gsub("_"," ") end;table.sort(labels)
  local text="BROKEN BONES | "..(#labels==0 and "No injuries" or table.concat(labels,", "))
  for _,id in ipairs(sdk.net.players()) do
    if tostring(id)~=tostring(sdk.net.info().local_id) then
      local p=sdk.net.read(id,"injuries")
      if type(p)=="table" and p.v==1 and type(p.j)=="table" and #p.j>0 then
        local skater=sdk.player.skater(id);text=text.."\n"..(skater and skater.name or tostring(id))..": "..#p.j.." injured joints"
      end
    end
  end
  if status~="" then text=text.."\n"..status end
  sdk.ui.text("bones",text:sub(1,1000))
end
local function fixed()
  local rig=sdk.rig.read({"tick","ragdoll","contacts"});local player=sdk.player.read();local now=sdk.time.elapsed
  if cfg("heal_on_teleport",true) and player.mode=="teleport" and last_mode~="teleport" then heal() end
  last_mode=player.mode
  local enabled=cfg("enabled",true)
  if was_enabled and not enabled then heal() end;was_enabled=enabled
  for i,j in pairs(pending) do
    local result=sdk.commands.result("joint_"..i)
    if result then
      pending[i]=nil
      if result.ok then broken[i]=j;status="Impact injured "..j.name:gsub("JOINT_",""):gsub("_"," ");publish()
      else status="Joint unchanged: "..tostring(result.error) end
    end
  end
  if not enabled or last_tick==rig.tick or sdk.player.attached() or player.suspended then draw();return end
  last_tick=rig.tick
  local joints
  local strongest={}
  for _,c in ipairs(rig.contacts or {}) do
    if c.phase~="end" and (c.closing_speed or 0)>=cfg("closing_speed",3) then
      if not joints then
        joints={};for _,j in ipairs(sdk.rig.joints()) do joints[j.child]=j end
      end
      for _,body in ipairs({c.a,c.b}) do
        if body and body.kind=="skater" then
          local j=joints[body.index];local impulse=length(c.impulse)
          if eligible(j) and impulse>=cfg("impact_impulse",80) and not broken[j.index] and not pending[j.index] and
            (not j.override_owner or j.override_owner==sdk.mod_id) and now-(last_hit[j.index] or -100)>=cfg("cooldown",0.75) then
            if not strongest[j.index] or impulse>strongest[j.index].impulse then strongest[j.index]={joint=j,impulse=impulse,at=now} end
          end
        end
      end
    end
  end
  for i,hit in pairs(strongest) do recent[i]=hit end
  for i,hit in pairs(recent) do
    if now-hit.at>0.3 then recent[i]=nil
    elseif not cfg("only_bails",false) or player.bailing or rig.ragdoll then
      sdk.commands.request("joint_"..i,{kind="player_joint",joint=i,options={free_swing=true,free_twist=true,
        drive_enabled=false,possession_enabled=false,descendants=true}})
      pending[i]=hit.joint;last_hit[i]=now;recent[i]=nil
    end
  end
  draw()
end
return {
 on_load=function()
  if (sdk.capabilities.player_physics or 0)<2 or not sdk.capabilities.command_results then error("Broken Bones requires the generalized native rig API build") end
  menu();publish();draw()
 end,
 on_fixed_update=fixed,
 on_event=function(e)
   if e.name=="world_changed" then broken,pending,recent,last_hit={},{},{},{};last_tick=nil;status="";publish();menu()
   elseif e.name=="menu_action" and e.menu=="bones" and e.item=="heal" then heal();draw() end
 end,
 on_unload=function() heal();sdk.net.publish("injuries",nil);sdk.ui.remove_menu("bones");sdk.ui.text("bones","") end,
}
