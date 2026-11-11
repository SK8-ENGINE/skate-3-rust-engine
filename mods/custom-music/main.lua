-- Custom Music: your playlist instead of the soundtrack, or mixed with it (README.md).
local started, checked, wait = false, false, 0

local function setting(name, default)
  local v = sdk.settings and sdk.settings[name]
  if v == nil then return default end
  return v
end

local function start()
  local mix = setting("mode", "mix") == "mix"
  sdk.music.play_playlist(setting("playlist", "playlists/my-music.json"), {soundtrack = mix, shuffle = setting("shuffle", true)})
  local volume = setting("volume", 1)
  if volume ~= 1 then sdk.music.set_volume(volume) end
  if not setting("caption", true) then sdk.music.caption{enabled = false} end
  sdk.log("CUSTOM_MUSIC playlist " .. tostring(setting("playlist", "")) .. (mix and " mixed with the soundtrack" or " replacing the soundtrack"))
end

return {
  on_update = function(event)
    if checked then return end
    if not started then
      start()
      started, wait = true, 3
      return
    end
    wait = wait - ((event and tonumber(event.dt)) or 0)
    if wait > 0 then return end
    checked = true
    local s = sdk.music.state()
    if s.owner == nil or not s.on then
      sdk.log("CUSTOM_MUSIC the playlist did not load: check the Playlist setting and README.md (the game log has the reason)")
      sdk.ui.central_message("Custom Music: playlist not found", 6)
    end
  end,
  on_event = function(event)
    if type(event) == "table" and event.name == "music_track_started" then
      sdk.log(string.format("CUSTOM_MUSIC playing %s - %s", tostring(event.artist), tostring(event.title)))
    end
  end,
}
