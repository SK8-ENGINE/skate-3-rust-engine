-- All match rules and turn authority live here; the SDK is game-mode agnostic.
local WORD = "SKATE"
local game, running, serial = nil, false, 0
local received, received_host
local applied, released, ack, restore = nil, nil, nil, nil
local armed_at, last_publish, menu_stamp = 0, -1, nil
local function info() return sdk.net.info() end
local function me() return tostring(info().local_id or "0") end
local function host() return not info().active or info().is_host end
local function roster()
  local ids, have = {me()}, {[me()]=true}
  for _, peer in ipairs(sdk.net.players()) do
    local id=tostring(peer)
    if not have[id] then ids[#ids+1],have[id]=id,true end
  end
  table.sort(ids)
  -- The transport host starts, regardless of numeric/lexicographic peer order.
  local first=host() and me() or tostring(info().host_id)
  for i,id in ipairs(ids) do if id==first then table.remove(ids,i);table.insert(ids,1,id);break end end
  return ids
end
local function name(id)
  if id==me() then return "You" end
  local s=sdk.player.skater(id)
  return s and s.name or ("Player "..tostring(id))
end
local function canon(s) return tostring(s or ""):lower():gsub("[^%w]", "") end
local function allowed(t) local c=canon(t);return c~="" and (sdk.settings.allow_ollie or c~="ollie" and c~="nollie") end
local function index(ids,id) for i,p in ipairs(ids) do if p==id then return i end end return 0 end
local function count(s) local n=0;for _,id in ipairs(s.ids) do if (s.l[id] or 0)<5 then n=n+1 end end return n end
local function next_setter(s)
  local at=index(s.ids,s.a)
  for step=1,#s.ids do local id=s.ids[(at+step-1)%#s.ids+1];if (s.l[id] or 0)<5 then return id end end
  return s.ids[1]
end
local function next_copier(s)
  for _,id in ipairs(s.ids) do
    if (s.solo or id~=s.a) and (s.l[id] or 0)<5 and not s.d[id] and not s.f[id] then return id end
  end
end
local function publish(s)
  local l,d,f={},{},{}
  for _,id in ipairs(s.ids) do
    l[#l+1]=tostring(s.l[id] or 0);d[#d+1]=s.d[id] and "1" or "0";f[#f+1]=s.f[id] and "1" or "0"
  end
  -- Split immutable roster/start pose from turn state; each value stays <512B.
  sdk.net.publish("origin",{e=s.e,v=s.v,ids=s.ids,p=s.origin.position,h=s.origin.heading})
  sdk.net.publish("skate",{e=s.e,v=s.v,q=s.q,p=s.p,n=s.n,a=index(s.ids,s.a),c=index(s.ids,s.c),
    t=s.t,w=index(s.ids,s.w),g=s.g,r=math.ceil(math.max(0,(s.u or 0)-sdk.time.elapsed)),
    l=table.concat(l),d=table.concat(d),f=table.concat(f),error=s.error})
  last_publish=sdk.time.elapsed
end
local function read_game()
  if host() then return game end
  local peer=tostring(info().host_id)
  if received_host~=peer then received,received_host=nil,peer end
  local w,o=sdk.net.read(peer,"skate"),sdk.net.read(peer,"origin")
  if type(w)~="table" or type(o)~="table" then received=nil;return nil end
  if w.e~=o.e or w.v~=o.v then return received end
  if type(o.ids)~="table" or type(w.l)~="string" then return nil end
  local s={e=w.e,q=w.q,p=w.p,n=w.n,ids=o.ids,a=o.ids[w.a],c=o.ids[w.c],t=w.t,w=o.ids[w.w],g=w.g,r=w.r,error=w.error,
    origin={position=o.p,heading=o.h,velocity={0,0,0}},l={},d={},f={}}
  for i,id in ipairs(s.ids) do s.l[id]=tonumber(w.l:sub(i,i)) or 0;s.d[id]=tostring(w.d):sub(i,i)=="1";s.f[id]=tostring(w.f):sub(i,i)=="1" end
  received=s
  return s
end
local function release()
  if applied then
    sdk.player.suspend(false);sdk.camera.watch(nil)
    if restore then sdk.player.teleport(restore) end
    sdk.net.publish("ack",nil)
  end
  applied,released,ack,restore=nil,nil,nil,nil
end
local function control(s, fixed)
  if not s or s.p=="i" or s.p=="w" then release();return end
  local token=tostring(s.e)..":"..tostring(s.q)
  if applied~=token then
    if not restore then local p=sdk.player.read();restore={position=p.position,heading=p.heading,velocity={0,0,0}} end
    applied,released,ack=token,nil,nil
    sdk.player.suspend(true)
    sdk.camera.watch(s.c~=me() and s.c or nil)
  end
  if fixed and not ack then
    -- Acknowledge the suspension on the next command frame.
    ack={e=s.e,q=s.q,stage=0}
    sdk.net.publish("ack",ack)
  end
  if s.g and s.c==me() and released~=token then
    sdk.player.teleport(s.origin)
    sdk.player.suspend(false)
    sdk.camera.watch(nil)
    released,armed_at=token,sdk.time.elapsed+0.35
  end
  if fixed and released==token and ack and ack.stage==0 and sdk.time.elapsed>=armed_at then
    local p=sdk.player.read()
    ack={e=s.e,q=s.q,stage=1,l=p.landing_seq or 0,b=p.bail_seq or 0}
    sdk.net.publish("ack",ack)
  end
end
local function turn(s,id,phase)
  s.c,s.p,s.q,s.g,s.base=id,phase,s.q+1,false,nil
  s.u=sdk.time.elapsed+15
end
local function begin_set(s,id)
  s.a,s.t,s.d,s.f=id,"",{},{}
  turn(s,id,"s")
end
local function result(s)
  local id=next_copier(s)
  if id then turn(s,id,"c") else s.p,s.g,s.u="r",false,sdk.time.elapsed+2;s.q=s.q+1 end
end
local function read_ack(id) if id==me() then return ack end return sdk.net.read(id,"ack") end
local function matches(a,s) return type(a)=="table" and a.e==s.e and a.q==s.q end
local function host_tick()
  local s,now=game,sdk.time.elapsed
  if not s or not running then return end
  local present={};for _,id in ipairs(roster()) do present[id]=true end
  local changed=false
  for i=#s.ids,1,-1 do if not present[s.ids[i]] then table.remove(s.ids,i);changed=true end end
  if changed then
    s.v=s.v+1
    if not present[s.a] then begin_set(s,next_setter(s))
    elseif not present[s.c] and s.p=="c" then result(s) end
    publish(s)
  end
  if s.p=="r" and now>=s.u then
    if not s.solo and count(s)<=1 then
      s.p,s.g="w",false
      for _,id in ipairs(s.ids) do if (s.l[id] or 0)<5 then s.w=id end end
      running=false
    else s.n=s.n+1;begin_set(s,next_setter(s)) end
    publish(s);return
  end
  if s.p=="s" or s.p=="c" then
    if not s.g then
      local all=true
      for _,id in ipairs(s.ids) do if not matches(read_ack(id),s) then all=false end end
      if all then s.g=true;s.u=now+15;publish(s)
      elseif now>=s.u then s.p,s.error,running="i","A player did not acknowledge the turn. Enable the same mod on every player and restart.",false;publish(s) end
      return
    end
    if not s.base then
      local a=read_ack(s.c)
      if matches(a,s) and a.stage==1 then s.base={landing=a.l,bail=a.b};s.u=now+(s.p=="s" and 90 or (sdk.settings.copy_seconds or 45));publish(s)
      elseif now>=s.u then s.p,s.error,running="i","Active player did not finish resetting. Restart the session.",false;publish(s) end
      return
    end
    local p=sdk.player.skater(s.c)
    if p then
      local landed,bail=p.landing_seq or 0,p.bail_seq or 0
      local failed=bail>s.base.bail or now>=s.u
      local trick=landed>s.base.landing and not p.bailing and tostring(p.landed_trick or "") or nil
      if s.p=="s" then
        if failed then begin_set(s,next_setter(s));publish(s)
        elseif trick and allowed(trick) then s.t=trick;result(s);publish(s)
        elseif trick then s.base.landing=landed end
      elseif failed or trick then
        if not failed and canon(trick)==canon(s.t) then s.d[s.c]=true
        else s.f[s.c]=true;s.l[s.c]=math.min(5,(s.l[s.c] or 0)+1) end
        result(s);publish(s)
      end
    elseif now>=s.u then s.p,s.error,running="i","Active player observations are missing. Restart the session.",false;publish(s) end
  end
  if now-last_publish>=0.5 then publish(s) end
end
local function menu(force)
  local stamp=tostring(host())..tostring(running)
  if not force and stamp==menu_stamp then return end
  menu_stamp=stamp
  sdk.ui.menu("session",{title="SKATE",section="Gamemodes",items={
    {id="start",label="Start session",enabled=host() and not running,description=host() and "Host starts here. Other players take turns from this position." or "Only the multiplayer host can start this session."},
    {id="stop",label="Stop session",enabled=host() and running,description="End the match and restore everyone's skater and camera."},
  }})
end
local function overlay(s)
  local status,board="No host match received. Ask the host to start SKATE; all players must enable the same mod version.",{}
  if s then
    for _,id in ipairs(s.ids) do local l=WORD:sub(1,s.l[id] or 0);board[#board+1]=name(id)..(id==s.c and " > " or " ")..(l~="" and l or "-") end
    if s.p=="i" then status=s.error or "Session stopped. Open Gamemodes > SKATE to start."
    elseif s.p=="w" then status=s.w and name(s.w).." wins!" or "Session finished."
    elseif s.p=="r" then status="Round complete: "..s.t
    elseif not s.g then status="Preparing "..name(s.c).."'s turn; synchronizing players."
    elseif s.p=="s" then status=name(s.c)..": land a trick to set it."
    else status=name(s.c)..": copy "..s.t.." ("..tostring(s.r or math.ceil(math.max(0,s.u-sdk.time.elapsed))).."s)" end
    if s.c~=me() and s.p~="i" and s.p~="w" then status=status.."  Spectating." end
  end
  sdk.ui.text("skate_title","GAME OF SKATE")
  sdk.ui.text("skate_status",status)
  sdk.ui.text("skate_board",table.concat(board,"   "))
  sdk.ui.text("skate_last","Last landed trick: "..tostring(sdk.player.read().landed_trick or ""))
  sdk.ui.text("skate_net",tostring(info().status or ""))
end
return {
  on_load=function()
    assert((sdk.capabilities.player_control or 0)>=1 and (sdk.capabilities.camera or 0)>=3,"Update the game: player suspension and peer camera streaming are required")
    menu(true)
  end,
  on_ui_update=function()
    if not host() then running=false;game=nil end
    local s=read_game();control(s,false);overlay(s);menu()
  end,
  on_fixed_update=function()
    local s=read_game();control(s,true)
    if host() then host_tick() end
    s=read_game();control(s,false);overlay(s);menu()
  end,
  on_event=function(event)
    if event.name=="world_changed" then release();received,received_host=nil,nil;game,running=nil,false;sdk.net.publish("skate",nil);sdk.net.publish("origin",nil);menu(true)
    elseif event.name=="menu_action" and event.menu=="session" and host() then
      if event.item=="start" and not running then
        release();serial=serial+1
        local p=sdk.player.read()
        game={e=math.floor(sdk.time.elapsed*1000)*1000+serial,v=1,q=0,n=1,ids=roster(),l={},w=nil,
          origin={position=p.position,heading=p.heading or 0,velocity={0,0,0}}}
        game.solo=#game.ids==1;running=true;begin_set(game,me());publish(game);control(game,false);menu()
      elseif event.item=="stop" and game then game.p,game.g,running="i",false,false;publish(game);release();menu() end
    end
  end,
  on_unload=function()
    release();sdk.net.publish("skate",nil);sdk.net.publish("origin",nil);sdk.ui.remove_menu("session")
    for _,key in ipairs({"skate_title","skate_status","skate_board","skate_last","skate_net"}) do sdk.ui.text(key,"") end
  end,
}
