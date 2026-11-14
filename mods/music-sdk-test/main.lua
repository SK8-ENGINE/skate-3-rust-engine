-- Dev only (doc 11 "Music banks", sdk.music). Walks the music API once and logs MUSIC_TEST lines:
-- `MUSIC_TEST <check> ok|fail <details>` per check, `MUSIC_TEST event ...` per music_track_started.
-- Settings: the playlist's first two titles (make_bank.py prints them) and seconds to listen per step.
local step, wait, started, pass, fail, heard = 0, 0, {}, 0, 0, 0

local function log(text) sdk.log("MUSIC_TEST " .. text) end
local function check(name, ok, details)
  if ok then pass = pass + 1 else fail = fail + 1 end
  log(string.format("%s %s %s", name, ok and "ok" or "fail", details or ""))
end
local function state() return sdk.music.state() end
local function playing_title()
  local p = sdk.music.playing()
  return p and p.title or "nil"
end
local function setting(name, default)
  local v = sdk.settings and sdk.settings[name]
  if v == nil then return default end
  return v
end

return {
  on_update = function(event)
    local dt = (event and tonumber(event.dt)) or 0
    heard = heard + dt
    wait = wait - dt
    if wait > 0 then return end
    wait = 0.5
    local first, second = setting("first_title", "High Shimmer"), setting("second_title", "Low Hum")
    local listen = setting("step_seconds", 0)
    if step == 0 then
      check("capability", (sdk.capabilities.music or 0) >= 1, tostring(sdk.capabilities.music))
      sdk.music.caption{hold = 4, fade_in = 0.3, fade_out = 0.6, title_size = 36, bottom = 14}
      sdk.music.play_playlist("music/playlist.json")
      sdk.music.set_volume(0.6)
      step = 1
    elseif step == 1 then
      if #started >= 1 and heard >= listen then
        local s = state()
        check("playlist", started[1].title == first and started[1].custom == true, started[1].title)
        check("owner", s.owner ~= nil and s.on == true, tostring(s.owner))
        check("volume", math.abs((s.volume or 0) - 0.6) < 1e-3, tostring(s.volume))
        check("queued", (s.queued or 0) >= 2, "queued=" .. tostring(s.queued) .. " (bank tracks + 1 soundtrack)")
        sdk.music.skip()
        step, wait = 2, 0
      end
    elseif step == 2 then
      if #started >= 2 and heard >= listen then
        check("skip", started[2].title == second, started[2].title)
        local before = state().queued or 0
        sdk.music.queue_playlist("music/playlist.json")
        step, wait = 3, 1
        log("queued before=" .. before)
      end
    elseif step == 3 then
      check("queue", (state().queued or 0) >= 4, "queued=" .. tostring(state().queued))
      check("playing", playing_title() == second, playing_title())
      sdk.music.set_volume(nil)
      sdk.music.stop()
      step, wait = 4, 1
    elseif step == 4 then
      local s = state()
      check("stop", s.on == false, "on=" .. tostring(s.on))
      check("volume_reset", s.volume == 1.0, tostring(s.volume))
      sdk.music.play_soundtrack(nil)
      sdk.music.caption(nil)
      step, wait = 5, 1
    elseif step == 5 then
      local s = state()
      check("soundtrack", s.owner ~= nil, "on=" .. tostring(s.on) .. " owner=" .. tostring(s.owner))
      log(string.format("done pass=%d fail=%d", pass, fail))
      step = 6
    end
  end,
  on_event = function(event)
    if type(event) == "table" and event.name == "music_track_started" then
      started[#started + 1] = event
      heard = 0
      log(string.format("event started title=%s artist=%s custom=%s", tostring(event.title), tostring(event.artist), tostring(event.custom)))
    end
  end,
}
