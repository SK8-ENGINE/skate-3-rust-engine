-- Host-authoritative race to copy; only settled engine results count.
local POOL = {
  { name = "Kickflip", cat = "flip",     hint = "RS down, flick UP+RIGHT", ghint = "RS down, flick UP+LEFT",   color = { 0.23, 0.79, 0.98, 1 } },
  { name = "Heelflip", cat = "flip",     hint = "RS down, flick UP+LEFT",  ghint = "RS down, flick UP+RIGHT",  color = { 1.00, 0.62, 0.15, 1 } },
  { name = "Pop Shuvit", cat = "shuvit",   hint = "RS scoop RIGHT side",     ghint = "RS scoop LEFT side",       color = { 0.70, 0.50, 1.00, 1 } },
  { name = "360 Flip", cat = "flip",     hint = "RS scoop L -> DOWN -> UR", ghint = "RS scoop R -> DOWN -> UL", color = { 1.00, 0.84, 0.20, 1 } },
  { name = "Nose Grab", cat = "grab",    hint = "AIR: hold LT + RS DOWN",  ghint = "AIR: hold RT + RS DOWN",   color = { 0.35, 0.95, 0.45, 1 } },
  { name = "Ollie", cat = "ollie",        hint = "RS down, pop straight UP", ghint = "RS down, pop straight UP", color = { 0.75, 0.85, 1.00, 1 } },
  { name = "Seatbelt", cat = "grab",     hint = "AIR: hold LT + RS UP",    ghint = "AIR: hold RT + RS UP",     color = { 1.00, 0.45, 0.75, 1 } },
  { name = "Nollie", cat = "ollie",       hint = "RS tap UP then DOWN",     ghint = "RS tap UP then DOWN",      color = { 0.80, 0.85, 0.90, 1 } },
  { name = "Varial Kickflip", cat = "flip", hint = "RS down-LEFT to UP+RIGHT", ghint = "RS down-RIGHT to UP+LEFT", color = { 0.20, 0.90, 0.80, 1 } },
  { name = "Varial Heelflip", cat = "flip", hint = "RS down-RIGHT to UP+LEFT", ghint = "RS down-LEFT to UP+RIGHT", color = { 1.00, 0.45, 0.30, 1 } },
  { name = "360 Pop Shuvit", cat = "shuvit", hint = "RS scoop L,DOWN,RIGHT",  ghint = "RS scoop R,DOWN,LEFT",    color = { 0.85, 0.70, 1.00, 1 } },
  { name = "FS Pop Shuvit", cat = "shuvit", hint = "RS scoop LEFT side",     ghint = "RS scoop RIGHT side",      color = { 0.55, 0.65, 1.00, 1 } },
  { name = "Double Kickflip", cat = "flip", hint = "Flick, hold to keep flipping", ghint = "Flick, hold to keep flipping", color = { 0.30, 0.70, 1.00, 1 } },
  { name = "Double Heelflip", cat = "flip", hint = "Flick, hold to keep flipping", ghint = "Flick, hold to keep flipping", color = { 1.00, 0.55, 0.20, 1 } },
  { name = "Nollie Kickflip", cat = "flip", hint = "Nollie: flick to heelside", ghint = "Nollie: flick to heelside", color = { 0.45, 0.85, 0.90, 1 } },
  { name = "Nollie Heelflip", cat = "flip", hint = "Nollie: flick to toeside", ghint = "Nollie: flick to toeside", color = { 0.90, 0.60, 0.35, 1 } },
  { name = "Hardflip", cat = "flip", hint = "FS shuvit + kickflip. Awkward!", ghint = "FS shuvit + kickflip. Awkward!", color = { 1.00, 0.30, 0.45, 1 } },
  { name = "Laser Flip", cat = "flip", hint = "FS 360 shuvit + heelflip", ghint = "FS 360 shuvit + heelflip", color = { 0.60, 1.00, 0.40, 1 } },
  { name = "Inward Heelflip", cat = "flip", hint = "Shuvit + heelflip", ghint = "Shuvit + heelflip", color = { 1.00, 0.50, 0.60, 1 } },
  { name = "360 Hardflip", cat = "flip", hint = "FS 360 shuvit + kickflip", ghint = "FS 360 shuvit + kickflip", color = { 0.95, 0.35, 0.55, 1 } },
  { name = "Melon", cat = "grab", hint = "Grab BS, push board forwards", ghint = "Grab BS, push board forwards", color = { 0.40, 0.90, 0.50, 1 } },
  { name = "Method", cat = "grab", hint = "Grab and tweak it out", ghint = "Grab and tweak it out", color = { 0.50, 1.00, 0.60, 1 } },
  { name = "Mute", cat = "grab", hint = "Board sideways, front grab", ghint = "Board sideways, front grab", color = { 0.65, 0.95, 0.45, 1 } },
  { name = "Stale", cat = "grab", hint = "Board sideways, back grab", ghint = "Board sideways, back grab", color = { 0.75, 0.90, 0.40, 1 } },
  { name = "Crail", cat = "grab", hint = "Nose reach, watch the hand", ghint = "Nose reach, watch the hand", color = { 0.45, 0.85, 0.55, 1 } },
  { name = "Tail Grab", cat = "grab", hint = "Push forward, back-hand grab", ghint = "Push forward, back-hand grab", color = { 0.55, 0.80, 0.50, 1 } },
  { name = "50-50", cat = "grind", fam = true, hint = "Both trucks. Line up rail", ghint = "Both trucks. Line up rail", color = { 0.90, 0.90, 0.95, 1 } },
  { name = "Smith Grind", cat = "grind", fam = true, hint = "Line up Smith and grind", ghint = "Line up Smith and grind", color = { 0.88, 0.86, 0.90, 1 } },
  { name = "Krooked", cat = "grind", fam = true, hint = "Nose into it, crooked grind", ghint = "Nose into it, crooked grind", color = { 0.86, 0.80, 0.92, 1 } },
  { name = "Nose Blunt", cat = "grind", fam = true, hint = "Nose up onto the blunt", ghint = "Nose up onto the blunt", color = { 0.84, 0.82, 0.94, 1 } },
  { name = "Feeble", cat = "grind", fam = true, hint = "Front truck hangs feeble", ghint = "Front truck hangs feeble", color = { 0.82, 0.86, 0.92, 1 } },
  { name = "Salad", cat = "grind", fam = true, hint = "Salad grind, hold it", ghint = "Salad grind, hold it", color = { 0.80, 0.90, 0.88, 1 } },
  { name = "Blunt Slide", cat = "grind", fam = true, hint = "Up on the blunt, slide it", ghint = "Up on the blunt, slide it", color = { 0.87, 0.83, 0.93, 1 } },
  { name = "Willie", cat = "grind", fam = true, hint = "Willie grind, steezy", ghint = "Willie grind, steezy", color = { 0.83, 0.89, 0.91, 1 } },
  { name = "Over-Krooks", cat = "grind", fam = true, also = { "overkrook", "overkrooked", "krooks" }, hint = "Over the krooks", ghint = "Over the krooks", color = { 0.85, 0.81, 0.93, 1 } },
  { name = "Nose Manual", ground = true, cat = "manual", fam = true, hint = "Push partway, sweet spot", ghint = "Push partway, sweet spot", color = { 0.70, 0.95, 0.85, 1 } },
  { name = "BS Fastplant", ground = true, cat = "manual", fam = true, hint = "BS grab, push off back foot", ghint = "BS grab, push off back foot", color = { 0.95, 0.75, 0.45, 1 } },
  { name = "BS Handplant", ground = true, cat = "manual", fam = true, hint = "Ramp angle, hold at edge", ghint = "Ramp angle, hold at edge", color = { 0.93, 0.70, 0.50, 1 } },
  { name = "FS 360 Pop Shuvit", cat = "shuvit", hint = "Find the start spot", ghint = "Find the start spot", color = { 0.65, 0.60, 1.00, 1 } },
  { name = "Nollie 360 Flip", cat = "flip", hint = "Nollie shuvit + kickflip", ghint = "Nollie shuvit + kickflip", color = { 0.50, 0.80, 0.95, 1 } },
  { name = "FS Fastplant", ground = true, cat = "manual", fam = true, hint = "FS grab, push off front foot", ghint = "FS grab, push off front foot", color = { 0.94, 0.78, 0.42, 1 } },
  { name = "Triple Kickflip", cat = "flip", hint = "Flick, hold to keep flipping", ghint = "Flick, hold to keep flipping", color = { 0.25, 0.65, 1.00, 1 } },
  { name = "Triple Heelflip", cat = "flip", hint = "Flick, hold to keep flipping", ghint = "Flick, hold to keep flipping", color = { 1.00, 0.50, 0.25, 1 } },
  { name = "360 Spin", cat = "spin", spin = 360, hint = "Spin 360 either way, stomp it", ghint = "Spin 360 either way, stomp it", color = { 0.40, 1.00, 0.70, 1 } },
  { name = "540 Spin", cat = "spin", spin = 540, hint = "Spin 540 either way, stomp it", ghint = "Spin 540 either way, stomp it", color = { 0.45, 0.95, 0.75, 1 } },
  { name = "720 Spin", cat = "spin", spin = 720, hint = "Spin 720 either way, stomp it", ghint = "Spin 720 either way, stomp it", color = { 0.50, 0.90, 0.80, 1 } },
  { name = "Kickflip 180", cat = "combo", combo = { flip = "KICKFLIP", pool = 1, spin = 180 }, hint = "Kickflip + spin 180 either way", ghint = "Kickflip + spin 180 either way", color = { 0.30, 0.75, 1.00, 1 } },
  { name = "Kickflip 360", cat = "combo", combo = { flip = "KICKFLIP", pool = 1, spin = 360 }, hint = "Kickflip + spin 360 either way", ghint = "Kickflip + spin 360 either way", color = { 0.28, 0.72, 1.00, 1 } },
  { name = "Heelflip 180", cat = "combo", combo = { flip = "HEELFLIP", pool = 2, spin = 180 }, hint = "Heelflip + spin 180 either way", ghint = "Heelflip + spin 180 either way", color = { 1.00, 0.58, 0.22, 1 } },
  { name = "Heelflip 360", cat = "combo", combo = { flip = "HEELFLIP", pool = 2, spin = 360 }, hint = "Heelflip + spin 360 either way", ghint = "Heelflip + spin 360 either way", color = { 1.00, 0.55, 0.20, 1 } },
  { name = "Ollie 180", cat = "combo", combo = { flip = "OLLIE", pool = 6, spin = 180 }, hint = "Ollie + spin 180 either way", ghint = "Ollie + spin 180 either way", color = { 0.72, 0.82, 1.00, 1 } },
  { name = "Ollie 360", cat = "combo", combo = { flip = "OLLIE", pool = 6, spin = 360 }, hint = "Ollie + spin 360 either way", ghint = "Ollie + spin 360 either way", color = { 0.70, 0.80, 1.00, 1 } },
  { name = "360 Flip 180", cat = "combo", combo = { flip = "360_FLIP", pool = 4, spin = 180 }, hint = "Treflip + spin 180 either way", ghint = "Treflip + spin 180 either way", color = { 0.95, 0.80, 0.25, 1 } },
  { name = "Melon 360", cat = "combo", combo = { flip = "MELON", pool = 21, spin = 360 }, hint = "Melon + spin 360 either way", ghint = "Melon + spin 360 either way", color = { 0.42, 0.88, 0.52, 1 } },
  { name = "Nose Grab 360", cat = "combo", combo = { flip = "NOSE_GRAB", pool = 5, spin = 360 }, hint = "Nose grab + spin 360 either way", ghint = "Nose grab + spin 360 either way", color = { 0.38, 0.92, 0.48, 1 } },
  { name = "Method 360", cat = "combo", combo = { flip = "METHOD", pool = 22, spin = 360 }, hint = "Method + spin 360 either way", ghint = "Method + spin 360 either way", color = { 0.52, 0.98, 0.62, 1 } },
  { name = "Superdude", cat = "grab", hint = "Grab + superman out huge", ghint = "Grab + superman out huge", color = { 0.60, 0.90, 0.50, 1 } },
  { name = "Miracle Whip", cat = "grab", hint = "Whip it. It's a grab.", ghint = "Whip it. It's a grab.", color = { 0.65, 0.88, 0.48, 1 } },
  { name = "Late Kickflip", cat = "flip", hint = "Ollie first, flip late", ghint = "Ollie first, flip late", color = { 0.32, 0.72, 1.00, 1 } },
  { name = "Late Heelflip", cat = "flip", hint = "Ollie first, flip late", ghint = "Ollie first, flip late", color = { 1.00, 0.57, 0.23, 1 } },
  { name = "Late Shuvit", cat = "shuvit", hint = "Ollie first, shuvit late", ghint = "Ollie first, shuvit late", color = { 0.72, 0.52, 1.00, 1 } },
  { name = "Kickflip Underflip", cat = "flip", hint = "Flip it, then flip it back", ghint = "Flip it, then flip it back", color = { 0.34, 0.70, 1.00, 1 } },
  { name = "Heelflip Underflip", cat = "flip", hint = "Flip it, then flip it back", ghint = "Flip it, then flip it back", color = { 1.00, 0.53, 0.25, 1 } },
  { name = "Nollie Pop Shuvit", cat = "shuvit", hint = "Nollie stance shuvit", ghint = "Nollie stance shuvit", color = { 0.68, 0.62, 1.00, 1 } },
  { name = "FS Fastplant", ground = true, cat = "manual", fam = true, hint = "FS grab, push front foot", ghint = "FS grab, push front foot", color = { 0.94, 0.76, 0.44, 1 } },
  { name = "Airwalk", cat = "grab", hint = "Airwalk it out", ghint = "Airwalk it out", color = { 0.62, 0.89, 0.49, 1 } },
  { name = "Coffin", ground = true, cat = "grab", hint = "Lay back, feet up. Coffin", ghint = "Lay back, feet up. Coffin", color = { 0.58, 0.86, 0.52, 1 } },
  { name = "Cross Bone", cat = "grab", hint = "Cross-bone grab", ghint = "Cross-bone grab", color = { 0.66, 0.87, 0.47, 1 } },
  { name = "Caveman", ground = true, cat = "ollie", hint = "Step off, jump back on", ghint = "Step off, jump back on", color = { 0.74, 0.84, 1.00, 1 } },
  { name = "Acid Drop", ground = true, cat = "ollie", hint = "Ride off a drop", ghint = "Ride off a drop", color = { 0.73, 0.83, 1.00, 1 } },
}

local game, received, epoch = nil, nil, 0
local last_publish, menu_stamp, watching = -1, nil, false
local deck = {}
local acknowledged
local function option(k,d) local v=sdk.settings[k];if v==nil then return d end;return v end
local function net() return sdk.net.info() end
local function me() return tostring(net().local_id or "0") end
local function host() return not net().active or net().is_host end
local function now() return sdk.time.elapsed end
local function canon(s)
  s=tostring(s or ""):lower():gsub("id_trick_",""):gsub("_"," ")
  for _,category in ipairs({"ground","trick","flip","grab","grind","authentic","air","metrics"}) do
    s=s:gsub("%f[%w]"..category.."%f[%W]","")
  end
  s=s:gsub("%f[%w]switch%f[%W]",""):gsub("%f[%w]fakie%f[%W]","")
  return (s:gsub("[^%w]",""))
end
local aliases={stalegrab="stale",stalefish="stale",crailgrab="crail",seatbeltgrab="seatbelt",
  mutegrab="mute",superman="superdude",ngairwalk="airwalk",crossbone="crossbone",
  crooked="krooked",crookedgrind="krooked",krooks="krooked",nosebluntslide="noseblunt",
  popshoveit="popshuvit",shuvit="popshuvit",["360shuvit"]="360popshuvit"}
local function key(s) local k=canon(s);return aliases[k] or k end
local function matches(label,t,p)
  -- The scorer appends body rotation after the board/grab name. A leading
  -- 360 in "360 Flip" is a board trick, never evidence of a body spin.
  local spin=tonumber(tostring(label):match("%s(%d+)%s*$"))
  local base=tostring(label):gsub("%s%d+%s*$","")
  if p.landed_spin_degrees~=nil then spin=math.abs(p.landed_spin_degrees) end
  local raw=p.landed_trick_base or ""
  if base:lower():find("halfcab",1,true) then spin=180;base=base:gsub("[FfBb][Ss] ?[Hh]alf[Cc]ab","") end
  if t.spin then
    return spin==t.spin
  end
  if t.combo then
    return spin==t.combo.spin and (key(base:gsub("^[FfBb][Ss] ",""))==key(POOL[t.combo.pool].name) or raw~="" and key(raw)==key(POOL[t.combo.pool].name))
  end
  if t.cat=="grind" or t.cat=="grab" then base=base:gsub("^[FfBb][Ss] ","") end
  local k=key(base)
  if k==key(t.name) or raw~="" and key(raw)==key(t.name) then return true end
  for _,a in ipairs(t.also or {}) do if k==key(a) then return true end end
  return false
end
local function enabled(t)
  -- The extracted scoring catalog has no settled Caveman/Acid Drop labels.
  -- Keep their guide entries, but never issue an unconfirmable challenge.
  if t.name=="Caveman" or t.name=="Acid Drop" then return false end
  local keys={flip="allow_flips",shuvit="allow_shuvits",ollie="allow_ollies",grab="allow_grabs",grind="allow_grinds",manual="allow_manuals_plants",spin="allow_spins"}
  if t.combo then return option("allow_spins",true) and enabled(POOL[t.combo.pool]) end
  return option(keys[t.cat],true)
end
local function draw()
  if #deck==0 then
    local seen={}
    for i,t in ipairs(POOL) do if enabled(t) and not seen[t.name] then deck[#deck+1]=i;seen[t.name]=true end end
    for i=#deck,2,-1 do local j=math.random(i);deck[i],deck[j]=deck[j],deck[i] end
  end
  return table.remove(deck)
end
local function publish()
  if not game then sdk.net.publish("match",nil);return end
  local l={};for _,id in ipairs(game.ids) do l[#l+1]=tostring(game.l[id] or 0) end
  sdk.net.publish("match",{e=game.e,p=game.p,n=game.n,c=game.c,ids=game.ids,l=table.concat(l),
    r=math.ceil(math.max(0,game.until_at-now())),w=game.w or 0})
  last_publish=now()
end
local function read()
  if host() then return game end
  local w=sdk.net.read(tostring(net().host_id),"match")
  if type(w)~="table" or type(w.ids)~="table" or type(w.l)~="string" then received=nil;return nil end
  local s={e=w.e,p=w.p,n=w.n,c=w.c,ids=w.ids,l={},r=w.r,w=w.w}
  for i,id in ipairs(w.ids) do s.l[id]=tonumber(w.l:sub(i,i)) or 0 end
  received=s;return s
end
local function release()
  if watching then sdk.player.suspend(false);sdk.camera.watch(nil);watching=false end
end
local function controls(s)
  if s and s.p=="copy" then
    local token=tostring(s.e)..":"..s.n
    if acknowledged~=token then
      local p=sdk.player.read()
      sdk.net.publish("round",{e=s.e,n=s.n,l=p.landing_seq or 0,b=p.bail_seq or 0})
      acknowledged=token
    end
  end
  local out=s and s.p~="done" and s.p~="stopped" and (s.l[me()] or 0)<=0
  if not out then release();return end
  sdk.player.suspend(true);watching=true
  local target
  if option("spec_cam",true) then for _,id in ipairs(s.ids) do if (s.l[id] or 0)>0 then target=id;break end end end
  sdk.camera.watch(target)
end
local function menu()
  local s=read();local active=s and s.p~="done" and s.p~="stopped"
  local stamp=tostring(host())..tostring(active)
  if stamp==menu_stamp then return end;menu_stamp=stamp
  sdk.ui.menu("challenge",{title="Simon Says",section="Gamemodes",items={
    {id="start",label="Start match",enabled=host() and not active,description="Everyone races to land the called trick. First clean ride-away wins."},
    {id="stop",label="Stop match",enabled=host() and active},
  }})
end
local function next_round()
  game.n=game.n+1;game.c=draw()
  if not game.c then game.p="stopped";publish();return end
  game.p="countdown";game.until_at=now()+option("countdown_secs",3);game.w=0
  game.base={};game.pending={};publish()
end
local function start()
  epoch=epoch+1;deck={}
  local ids,have={me()},{[me()]=true}
  for _,id in ipairs(sdk.net.players()) do id=tostring(id);if not have[id] then ids[#ids+1]=id;have[id]=true end end
  game={e=tostring(me())..":"..epoch..":"..now(),ids=ids,l={},n=0,until_at=now()}
  for _,id in ipairs(ids) do game.l[id]=option("lives",3) end
  next_round()
end
local function resolve(winner)
  game.w=winner or 0
  local cost=option("sudden_on",false) and game.n>=option("sudden_round",6) and option("sudden_cost",2) or 1
  local alive=0
  for i,id in ipairs(game.ids) do
    if i~=winner then game.l[id]=math.max(0,game.l[id]-cost) end
    if game.l[id]>0 then alive=alive+1 end
  end
  game.p=(alive==0 or #game.ids>1 and alive<=1) and "done" or "break"
  game.until_at=now()+math.max(1,option("intermission_base",4)*option("esc_dur",0.85)^(game.n-1));publish()
end
local function advance()
  if not game or game.p=="done" or game.p=="stopped" then return end
  local connected={[me()]=true};for _,id in ipairs(sdk.net.players()) do connected[tostring(id)]=true end
  for _,id in ipairs(game.ids) do if not connected[id] then game.l[id]=0 end end
  if game.p=="countdown" and now()>=game.until_at then
    game.p="copy";game.until_at=now()+math.max(5,option("round_time",45)*option("esc_dur",0.85)^(game.n-1))
    game.base={}
    publish()
  elseif game.p=="break" and now()>=game.until_at then next_round()
  elseif game.p=="copy" then
    local ride=math.max(0.5,option("ride_away",2)-option("esc_ride",0.2)*(game.n-1))
    for i,id in ipairs(game.ids) do
      local p=sdk.player.skater(id);local b=game.base[id]
      if not b then
        local ack=sdk.net.read(id,"round")
        if type(ack)=="table" and ack.e==game.e and ack.n==game.n and type(ack.l)=="number" and type(ack.b)=="number" then
          b={l=ack.l,b=ack.b};game.base[id]=b
        end
      end
      if p and b and game.l[id]>0 then
        local landed,bail=p.landing_seq or 0,p.bail_seq or 0
        if bail~=b.b or p.bailing or p.on_board==false then game.pending[id]=nil end
        if landed>b.l and bail==b.b and not p.bailing and (p.landed_clean==nil and p.clean~=false or p.landed_clean==true) and not (p.landed_sketchy or p.sketchy) and matches(p.landed_trick,POOL[game.c],p) then
          game.pending[id]={at=now()+ride,b=bail}
        end
        b.l,b.b=landed,bail
        local pending=game.pending[id]
        if pending and now()>=pending.at and bail==pending.b and p.on_board~=false and not p.bailing then resolve(i);return end
      else game.pending[id]=nil end
    end
    if now()>=game.until_at then resolve(nil);return end
  end
  if now()-last_publish>=0.25 then publish() end
end
local card_token, card_at = nil, 0
local function card(s)
  local t=s and POOL[s.c]
  local token=s and tostring(s.e)..":"..s.n or "idle"
  if token~=card_token then card_token,card_at=token,now() end
  local accent=t and t.color or {0.23,0.79,0.98,1}
  local pop=option("flash",true) and 1+math.max(0,0.15-(now()-card_at)*0.3) or 1
  local hint=t and (option("stance","regular")=="goofy" and t.ghint or t.hint) or "First clean ride-away wins the round."
  local remaining=s and (s.r or math.ceil(math.max(0,s.until_at-now()))) or 0
  local items={
    {key="back",type="rect",position={0,0},size={420,220},color={0.04,0.05,0.06,0.94}},
    {key="accent",type="rect",position={0,0},size={420,6},color=accent},
    {key="title",type="text",position={16,14},size={388,24},text="SIMON SAYS"..(s and " | ROUND "..s.n or ""),font_size=16,color=accent},
    {key="call",type="text",position={16,46},size={388,48},text=t and t.name or "Ready to play",font_size=28,color={1,1,1,1}},
    {key="hint",type="text",position={16,96},size={388,38},text=hint,font_size=14,color={0.85,0.9,0.95,1}},
    {key="state",type="text",position={16,140},size={388,24},text=s and (s.p.." | "..remaining.."s | Lives: "..(s.l[me()] or 0)) or "Open Gamemodes > Simon Says",font_size=17,color=accent},
    {key="help",type="text",position={16,182},size={388,22},text="Start / Stop: Gamemodes > Simon Says",font_size=13,color={0.6,0.65,0.7,1}},
  }
  sdk.ui.canvas("simon_card",{anchor="top_right",offset={24,24},size={420,220},scale=option("card_scale",1)*pop,visible=true,items=items})
end
local function hud()
  local s=read();controls(s);menu();card(s)
  if not option("show_hud",true) then sdk.ui.text("simon","");return end
  if not s then sdk.ui.text("simon","SIMON SAYS | Open Gamemodes > Simon Says. All players must enable this mod.");return end
  local t=POOL[s.c];local lines={"SIMON SAYS | Round "..s.n.." | "..s.p}
  if t then
    lines[#lines+1]="Call: "..t.name.." | "..tostring(s.r or math.ceil(math.max(0,s.until_at-now()))).."s"
    lines[#lines+1]=option("stance","regular")=="goofy" and t.ghint or t.hint
  end
  for i,id in ipairs(s.ids) do local p=sdk.player.skater(id);lines[#lines+1]=(id==me() and "You" or p and p.name or id)..": "..(s.l[id] or 0).." lives"..(s.w==i and " - round winner" or "") end
  if s.p=="done" then lines[#lines+1]="Match ended. Start again from Gamemodes > Simon Says." end
  sdk.ui.text("simon",table.concat(lines,"\n"))
end
return {
  on_load=function() menu();hud() end,
  on_fixed_update=function() if host() then advance() end;hud() end,
  on_event=function(e)
    if e.name=="world_changed" then release();game,received=nil,nil;publish();menu()
    elseif e.name=="menu_action" and e.menu=="challenge" and host() then
      if e.item=="start" and (not game or game.p=="done" or game.p=="stopped") then start()
      elseif e.item=="stop" and game then game.p="stopped";publish();release() end
      hud()
    end
  end,
  on_unload=function() release();sdk.net.publish("match",nil);sdk.ui.remove_menu("challenge");sdk.ui.remove("simon_card");sdk.ui.text("simon","") end,
}
