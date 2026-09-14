-- Game rules belong in Lua. The engine only publishes generic scoring events.
local WORD = "SKATE"
local seen, game = {}, nil
local last_publish = -1
local running = false
local menu_stamp

local function info() return sdk.net.info() end
local function me() return tostring(info().local_id or "0") end
local function roster()
  local ids, have = {}, {}
  for _, id in ipairs(sdk.net.players()) do
    id = tostring(id)
    if not have[id] then ids[#ids + 1], have[id] = id, true end
  end
  if not have[me()] then ids[#ids + 1] = me() end
  table.sort(ids)
  return ids
end
local function name(id)
  if id == me() then return "You" end
  local s = sdk.player.skater(id)
  return s and s.name or ("Player " .. tostring(id))
end
local function canon(s) return tostring(s or ""):lower():gsub("[^%w]", "") end
local function allowed(s)
  local c = canon(s)
  return c ~= "" and (sdk.settings.allow_ollie or c ~= "ollie" and c ~= "nollie")
end
local function alive(s)
  local ids = {}
  for _, id in ipairs(s.ids) do
    if (s.l[id] or 0) < 5 then ids[#ids + 1] = id end
  end
  return ids
end
local function next_setter(s)
  local ids = alive(s)
  for i, id in ipairs(ids) do if id == s.a then return ids[i % #ids + 1] end end
  return ids[1] or s.ids[1]
end
local function copying(s, id)
  return #s.ids == 1 or id ~= s.a
end

-- Sample everybody every tick, including spectators/setters. Joining or loading
-- establishes a baseline; old results can never satisfy a new turn.
local function events(ids)
  local out, present = {}, {}
  local skaters = sdk.player.skaters()
  for _, id in ipairs(ids) do
    present[id] = true
    local s = skaters[id]
    if s then
      local landing, bail = tonumber(s.landing_seq) or 0, tonumber(s.bail_seq) or 0
      local previous = seen[id]
      if previous then
        if bail > previous.bail then out[id] = {bail=true}
        elseif landing > previous.landing and not s.bailing then
          local trick = tostring(s.landed_trick or "")
          if trick ~= "" then out[id] = {trick=trick} end
        end
      end
      seen[id] = {landing=landing, bail=bail}
    end
  end
  for id in pairs(seen) do if not present[id] then seen[id] = nil end end
  return out
end

-- Compact per-roster strings keep ten-player state below the 512-byte Lua limit.
local function publish(s)
  local letters, done, failed = {}, {}, {}
  for _, id in ipairs(s.ids) do
    letters[#letters+1] = tostring(s.l[id] or 0)
    done[#done+1] = s.d[id] and "1" or "0"
    failed[#failed+1] = s.f[id] and "1" or "0"
  end
  sdk.net.publish("skate", {p=s.p, n=s.n, a=s.a, t=s.t, w=s.w,
    r=math.ceil(math.max(0, s.u-sdk.time.elapsed)), ids=s.ids,
    l=table.concat(letters), d=table.concat(done), f=table.concat(failed)})
  last_publish = sdk.time.elapsed
end
local function read_game()
  if info().is_host or not info().active then return game end
  local wire = sdk.net.read(tostring(info().host_id), "skate")
  if type(wire) ~= "table" or type(wire.ids) ~= "table" or type(wire.l) ~= "string" then return nil end
  local s = {p=wire.p, n=wire.n, a=wire.a, t=wire.t, w=wire.w, r=wire.r, ids=wire.ids, l={}, d={}, f={}}
  for i, id in ipairs(s.ids) do
    s.l[id] = tonumber(wire.l:sub(i,i)) or 0
    s.d[id] = tostring(wire.d):sub(i,i) == "1"
    s.f[id] = tostring(wire.f):sub(i,i) == "1"
  end
  return s
end
local function begin_set(s, setter)
  s.p, s.a, s.t, s.d, s.f = "s", setter, "", {}, {}
  s.u = sdk.time.elapsed + 90
end
local function fail(s, id)
  if not s.d[id] and not s.f[id] then
    s.f[id], s.l[id] = true, math.min(5, (s.l[id] or 0) + 1)
  end
end
local function stopped(ids)
  return {p="i",n=0,a=ids[1],t="",w="",ids=ids,l={},d={},f={},u=sdk.time.elapsed}
end
local function menu(force)
  local host = not info().active or info().is_host
  local stamp=tostring(host)..tostring(running)
  if not force and stamp==menu_stamp then return end
  menu_stamp=stamp
  sdk.ui.menu("session", {title="SKATE",section="Gamemodes",items={
    {id="start",label="Start session",enabled=host and not running,description=not host and "Only the multiplayer host can start this session. Ask the host to start it, or leave multiplayer for a solo drill." or (running and "A session is already running. Stop it before starting a new one." or "Start a new game; solo starts a set-and-match drill.")},
    {id="stop",label="Stop session",enabled=host and running,description=not host and "Only the multiplayer host can stop this session." or (running and "End the current game for everyone." or "No session is running.")},
  }})
end
local function host_tick(ids, observed)
  local now = sdk.time.elapsed
  if not game or game.p == "w" and now >= game.u then
    game = {n=1, ids=ids, l={}, w=""}
    begin_set(game, ids[1])
    publish(game)
    return
  end
  local s = game
  s.ids = ids
  local present = {}
  for _, id in ipairs(ids) do present[id] = true; s.l[id] = s.l[id] or 0 end
  for id in pairs(s.l) do if not present[id] then s.l[id], s.d[id], s.f[id] = nil, nil, nil end end
  if not present[s.a] then begin_set(s, next_setter(s)); publish(s); return end
  if s.p == "r" and now >= s.u then
    local live = alive(s)
    if #live == 0 or #ids > 1 and #live == 1 then
      s.p, s.w, s.u = "w", live[1] or "", now + 8
    else
      s.n = s.n + 1
      begin_set(s, next_setter(s))
    end
    publish(s)
    return
  end
  if s.p == "s" then
    local e = observed[s.a]
    if e and e.trick and allowed(e.trick) then
      s.p, s.t, s.d, s.f = "c", e.trick, {}, {}
      s.u = now + (sdk.settings.copy_seconds or 45)
      publish(s)
      return
    elseif now >= s.u or e and e.bail then
      begin_set(s, next_setter(s))
      publish(s)
      return
    end
  elseif s.p == "c" then
    local changed, pending = false, 0
    for _, id in ipairs(alive(s)) do
      if copying(s, id) and not s.d[id] and not s.f[id] then
        local e = observed[id]
        if e and e.trick and canon(e.trick) == canon(s.t) then s.d[id], changed = true, true
        elseif e or now >= s.u then fail(s, id); changed = true
        else pending = pending + 1 end
      end
    end
    if pending == 0 then s.p, s.u, changed = "r", now+2, true end
    if changed then publish(s); return end
  end
  if now-last_publish >= 1 then publish(s) end
end
local function overlay()
  local s, id = read_game(), me()
  local status, board = "Waiting for the host...", {}
  if s then
    for _, peer in ipairs(s.ids) do
      local letters = WORD:sub(1, s.l[peer] or 0)
      board[#board+1] = name(peer) .. (peer == s.a and " * " or " ") .. (letters ~= "" and letters or "-")
    end
    local remaining = s.r or math.ceil(math.max(0, (s.u or 0)-sdk.time.elapsed))
    if s.p == "i" then status = "Session stopped. Open Gamemodes > SKATE to start."
    elseif s.p == "s" then status = name(s.a) .. ": land a trick to set it."
    elseif s.p == "w" then status = s.w ~= "" and (name(s.w) .. " wins. Restarting shortly.") or "Drill finished. Restarting shortly."
    elseif s.d[id] then status = "Matched " .. s.t .. "."
    elseif s.f[id] then status = "Missed " .. s.t .. "."
    elseif copying(s, id) then status = "Copy: " .. s.t .. " (" .. remaining .. "s)"
    else status = "Set: " .. s.t .. ". Other skaters are copying." end
  end
  sdk.ui.text("skate_title", "GAME OF SKATE")
  sdk.ui.text("skate_status", status)
  sdk.ui.text("skate_board", table.concat(board, "   "))
  local last = tostring(sdk.player.read().landed_trick or "")
  sdk.ui.text("skate_last", "Last landed trick: " .. (last ~= "" and last or "(none)"))
  sdk.ui.text("skate_net", #roster() == 1 and "Solo drill: set a trick, then land it again." or "")
end
return {
  on_load = function()
    assert((sdk.capabilities.skater or 0) >= 4, "Update the game: confirmed scoring observations are required")
    menu(true)
    sdk.log("Game of SKATE ready; start it from Gamemodes > SKATE")
  end,
  on_ui_update = function()
    if info().active and not info().is_host then running=false;game=nil end
    menu()
  end,
  on_fixed_update = function()
    local ids = roster()
    local observed = events(ids)
    if not info().active or info().is_host then
      if running then host_tick(ids, observed)
      elseif not game or game.p ~= "i" or sdk.time.elapsed-last_publish>=1 then game=stopped(ids);publish(game) end
    else game=nil;running=false end
    menu()
    overlay()
  end,
  on_event = function(event)
    if event.name == "world_changed" then
      seen,game,running={},nil,false; sdk.net.publish("skate",nil);menu(true)
    elseif event.name == "menu_action" and event.menu == "session" then
      if info().active and not info().is_host then sdk.log("Only the host can start or stop this session");return end
      if event.item == "start" then
        seen,game,running={},nil,true
        host_tick(roster(),{})
        menu()
      elseif event.item == "stop" then
        running=false;game=stopped(roster());publish(game);menu()
      end
    end
  end,
  on_unload = function()
    for _, key in ipairs({"skate_title", "skate_status", "skate_board", "skate_last", "skate_net"}) do sdk.ui.text(key, "") end
    sdk.net.publish("skate", nil)
    sdk.ui.remove_menu("session")
  end,
}
