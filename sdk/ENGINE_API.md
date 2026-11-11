# Engine observations and controls (API 2)

The SDK exposes engine primitives. Game rules, turn order, trick matching, scoring
policies, and win conditions belong in Lua mods. No host API depends on a mod ID
or a particular game mode.

## Player and scoring observations

`sdk.player.read()` reads the local skater. `sdk.player.skaters()` returns a table
keyed by peer ID; `sdk.player.skater(id)` reads one entry. Entries include names,
pose, velocity, angular velocity, movement mode, grind, stance, score, and named
input intents. `sdk.net.players()` lists connected peers; their first skater
observation may arrive later.

- `trick` / `trick_seq`: latest trick **announcement**, including midair tricks.
- `landed_trick` / `landing_seq`: latest confirmed, banked on-board scoring
  sequence. The counter advances only after successful native scoring settlement,
  not on announcement, bail, offboard completion, or teleport. The label remains
  available until the next confirmed result.
- `bail_seq`: counter incremented once when entering a native wipeout.

Counters are observation cursors, not one-frame pulses. Remember the previous
counter, establish a baseline when a peer appears, and handle a lower counter as
a reset. Remote snapshots carry the latest state, not an unbounded event history;
intermediate events may be coalesced if several occur between received updates.
Trick labels are display strings, not a structured trick taxonomy. A mod chooses
its own matching policy. `sdk.capabilities.skater >= 4` includes these counters.

```lua
local previous = {}
return {on_fixed_update = function()
    for id, skater in pairs(sdk.player.skaters()) do
        local before = previous[id]
        if before and skater.landing_seq > before then
            sdk.log(skater.name .. " landed " .. skater.landed_trick)
        end
        previous[id] = skater.landing_seq
    end
end}
```

## Native player movement and session authority

`sdk.player.teleport({position={x,y,z}, heading=radians, velocity={x,y,z}, ground_fit=true, on_board=true})`
uses native player travel. Heading defaults to zero; velocity is optional. `ground_fit` (default true) puts the
arrival onto the ground below as Skate 3's teleports do (a ray from 0.75 m above, landing 0.15 m over the hit);
false places the skater exactly at `position`. `on_board` (default true) set to false lands the skater standing off
the board, so an idle skater stays where it lands (a held start for test runs or a scene). This
only moves the local skater. Coordinates are bounded to +/-100000 and velocity
components to +/-200. Invalid commands fail validation.

`sdk.session.info()` reports the transport host and current session authority.
Authority is global to the session, initially the host. The current authority may
`sdk.session.transfer(peer)` to another connected player; the transport host
ratifies the transfer asynchronously. The host can reclaim with
`sdk.session.claim()`. The current authority can use
`sdk.session.teleport(peer, options)` to relocate a connected skater. Receivers
validate the sender, authority generation, command sequence, and movement values.
Reconnect/host changes reset authority; the host reclaims it if its holder leaves.
These operations do not implement turns or game rules.

## Cameras and world volumes

`sdk.camera.watch(peer)` follows a remote skater's presented pose;
`sdk.camera.watch(nil)` releases spectating. Camera overrides retain the existing
single-mod ownership rule.

`sdk.volumes.box(key, {position={x,y,z}, size={x,y,z}, rotation={x,y,z,w},
visible=true, color={r,g,b}, opacity=0.3})` creates or updates an oriented box.
`sdk.volumes.read(key).inside` lists peer IDs whose observed root positions lie
inside it. This is a point overlap query, not a collider or a skater hull test.
Boxes belong to their creating mod, max 32 per mod; remove with
`sdk.volumes.remove(key)`. Changes become observable on the next snapshot.

## Minimap icons (`sdk.minimap`, version 1)

`sdk.minimap.icon(key, {position={x,y,z}, icon="race_gate", angle=0, scale=1, fade=true, max_distance=nil,
colour={r,g,b,a}})` creates or moves an icon on the in-game HUD minimap. `icon` is a fe_map_icons record name (the
disc's `race_gate`, `skater`, `teleport`, `pro`, ...; its texture and max distance come from that record); it defaults
to `race_gate`, as retail's own script icons do. `max_distance` overrides the record's (0 = the minimap's 1000).
Each mod's icons are one group, drawn after the groups added before it. Icons belong to their mod, max 32 per mod,
and go when the mod is disabled; remove one with `sdk.minimap.remove(key)`. Local display only: in a session each
peer's mods place their own icons.

`sdk.minimap.show(false)` hides the HUD minimap (retail's minimap show / hide messages); `sdk.minimap.show(true)`
takes the mod's hide back. It never shows a minimap the player turned off in Game Settings or one the HUD hides; while
any mod hides it, it stays hidden. A disabled mod's hide goes with it.

## Loading screen (`sdk.loading`, capability `loading` 2: timing, stat rows and events since 2)

The loading screen between maps (Skate 3's tips loader, front-end screen 52) picks a Coach Frank line or a hint each time it opens.
`sdk.loading.hint(key, {title=..., text=...})` and `sdk.loading.coach_frank(key, text)` add (or replace, by key) this
mod's lines; a string id (`ID_HINTS_TITLE_3`) is localised, any other text shows as written.
`sdk.loading.stat(key, {label=..., value=...})` adds a row to the Stats panel next to the player's career stats (the
panel shows 5 rows picked at random from them). `sdk.loading.remove(key)` drops one. `sdk.loading.retail(false)` leaves only mods' lines on a panel mods give lines for (the retail lines stay
on a panel no mod fills); `retail(true)` takes that back. Lines join the next time the screen opens.

`sdk.loading.hold(key)` keeps a running load's screen up after the engine's own loading is done, until
`sdk.loading.release(key)`, for at most 30 s; it never opens the screen by itself. Lines, the retail switch and holds
belong to their mod and go when it is disabled. The movie itself is replaced through the menu movie overrides
(`sdk.menus.movie`). Local display only: in a session each peer shows its own loading screen.

`sdk.loading.timing({show=4.5, outro=0.5})` sets how long a panel shows (1 to 120 s, retail 9.0) and how long its
outro takes (0 to 10 s, retail 0.5); a missing field keeps the retail value, `timing(nil)` drops the change, the
newest mod's timing wins and it goes with the mod. Every mod's `on_event` gets `{name="loading_opened", map=...}`
when the screen comes up and `{name="loading_closed", map=...}` when it has gone (`map` is the map being loaded,
nil when unknown).

## Trigger volumes (`sdk.triggers`, capability `triggers` 1)

The engine tracks the map's named trigger volumes the way retail Skate 3 does:
each fixed tick the local player (body `"player"`) and every tracked body get a
query cylinder (retail: radius 0.34 m, spanning the skater from just below the
feet to just above the head) which is tested against each volume's oriented box.
Changes post `on_event` events before `on_fixed_update`:

    {name="trigger_entered"|"trigger_exited", body="player"|"mod:<mod>:<key>",
     volume=<id>, volume_name=<short name>, group="challenge"|"stairs"|"camera",
     instance_id=<16 hex digits or nil>, link_guid=<16 hex digits or nil>}

Retail map volumes come from the converted map (`maps/<Map>.triggers`, written by
setup); their `id` is the retail instance id challenge scripts use (e.g.
SkateSchool's map-wide `tut_sksc_reset_vol01` is `2c7017060025128f`). Custom maps
carry their own in a `<Map>.triggers` sidecar or a `TVOL` extension (same JSON;
see `docs/hails-additions/24-trigger-volumes.md`).

- `sdk.triggers.list()`: every volume: `{id, name, full_name, group, source="map"|"mod",
  owner, instance_id, link_guid, center, axes, rotation, half_extents, fatness,
  aabb_min, aabb_max, enabled, inside={bodies}}`. `sdk.triggers.get(id_or_name)`.
- `sdk.triggers.inside(body)`: volume ids the body (default `"player"`) is in.
- `sdk.triggers.box(key, {center={x,y,z}, half_extents={x,y,z}, rotation={x,y,z,w},
  name="...", group="challenge"})`: a mod volume with id `mod:<mod>:<key>`
  (max 64 per mod); `sdk.triggers.remove(key)`.
- `sdk.triggers.set_enabled(id, false)` switches a map volume off for everyone
  until the same mod switches it on or stops.
- `sdk.triggers.track(key, {radius=0.34, length=0})` follows one of the mod's
  physics bodies (body id `mod:<mod>:<key>`, max 16); `sdk.triggers.untrack(key)`.
  The cylinder stands on the body's position (like the player's feet) and
  reaches `length` metres up, plus retail's pads (2 cm below, 8 cm above).
  Positions are taken after the dynamics step, so their events lag one tick.
- `sdk.triggers.configure({radius=, length_scale=, length_pad=, foot_pad=})` changes
  the query-cylinder constants (one mod at a time); `configure()` restores retail.
  The player's cylinder is built like retail's skater from its feet (ground
  point), head and hips: radius 0.34, half-height `length_scale * |head - feet| +
  length_pad` (0.5, 0.05), one end `foot_pad` (0.02) below the feet.
- `sdk.triggers.teleporter(key, {volume="<volume id or own box key>", exits={{position={x,y,z},
  heading=<radians>, map="<map name>", world="<world row>"}}, title="..."})` adds a teleporter like Skate 3's (max 64 per
  mod): entering the volume shows the prompt with `title`, D-pad Up asks to confirm (one exit: No /
  Yes; up to 8 exits: a list), and the skater lands at the exit, on another map after its loading
  screen. An exit's optional `world` names a skate.Park world on that map (`dist_megapark_full` = Stadium - Full): the map
  loads with that world's layout, as retail's park teleporters do. A `key` equal to a retail teleporter's id (`tele_world_to_stadium`) replaces it on that map;
  `indicator="teleport"` gives it Skate 3's floating icon (any `ribbon_indicator` row key), anchored at
  `indicator_position={x,y,z}` (default: the volume's centre) plus the row's offset, and `indicator_style="smallarrow"` picks another `visualindicators` look (`default`, `gates`, `trail`, `non_fade`, `lesswiggle`, `smallarrow_nonfade`, `clintjammie`) and `indicator_colour="red"` its colour (`default` = blue, `blue`, `red`, `green`, `yellow`, `orange`, `purple`, `domination`); without `indicator` it has none
  (a retail teleporter replaced by key keeps no icon unless the mod names one).
  `sdk.triggers.teleporter(key)` removes it. Every use posts `{name="teleporter_used", teleporter=,
  exit=<1-based>, body="player"}`. Mod teleporters go when the mod stops and on a map change (re-add
  them in `world_changed`).
- Only `challenge` volumes post `trigger_entered` / `trigger_exited`; retail's
  `stairs` and `camera` groups have no tracked bodies (they are listed, not tracked).

Removing a volume never posts an exit (retail); a body that stops being tracked
exits everything it was in. Everything a mod set is undone when it stops; mod
volumes, switches and tracked bodies are also cleared when the world changes
(re-add them on `world_changed`).

`sdk.camera.capture(key, {position={x,y,z}, look_at={x,y,z}, fov=radians,
width=256, height=256})` creates or updates an offscreen camera. FOV defaults to
70 degrees (in radians), range 0.2–2.5. Dimensions must be multiples of 16 between
64 and 512. Limits: 2 captures per mod, 4 overall. Bind a capture using
`sdk.graphics.mesh_buffer(key, {capture="camera-key"})`, then write mesh geometry
and UVs with the existing buffer methods. `texture` and `capture` are mutually
exclusive. Capture surfaces are excluded from capture views to prevent recursive
render-target sampling. Clearing/resizing a capture updates its bound materials.
Release it with `sdk.camera.clear_capture(key)`.

`sdk.graphics.mesh(key, {path="model.glb", opacity=0.5})` supports opacity 0–1,
including replication. Camera captures and volumes are local, mod-owned resources
and are cleared when their mod stops or the world changes.

## Challenges (`sdk.challenges`, capability `challenges` 1)

The retail challenges (own the spot so far) as the engine runs them. They only
run while the player has the **Challenges** opt-in on (Graphics overlay,
Opt-Ins, or `SKATE_CHALLENGES=1`); `sdk.challenges.on()` says whether it is.

    sdk.challenges.list()     -- this map's challenges: {id, family, map, title_id, title,
                              --   position, enabled, owned_points, killed_points,
                              --   completion="owned"|"killed"|nil, high_score, discovery=0|1|2}
    sdk.challenges.get(id)    -- one of them, or nil
    sdk.challenges.run()      -- the run going: {id, points, requirement, personal_best,
                              --   out_of_bounds=seconds left outside the area or nil}, or nil
    sdk.challenges.start(id)  -- sign the player up, as D-pad Up at its marker does
    sdk.challenges.abort()    -- end the run going (abandoned)
    sdk.challenges.set_enabled(id, false)  -- hide its sign-up and discovery until the mod stops
    sdk.challenges.configure(id, {owned_points=, killed_points=, out_of_bounds_time=,
                                  owned_display_time=, killed_display_time=})
                              -- override setup values from the next start; nil options clear

Events in `on_event`: `challenge_discovered`, `challenge_started`
(`{challenge=id}`), `challenge_personal_best` (`{challenge, points}`),
`challenge_ended` (`{challenge, points, completion="abandoned"|"owned"|"killed"}`).
Discovery is 0 not discovered, 1 discovered, 2 intro seen (retail's statuses).
Everything a mod hid or overrode is undone when it stops; a run already going
keeps the values it started with. Details:
`docs/hails-additions/42-challenges.md#mod-entry`.

## Career (`sdk.career`, capability `career` 1)

The career that runs with the Challenges opt-in: board sales, money, milestones,
the progression actions, flags, the team and achievements.

    sdk.career.on()           -- false without the opt-in or the progression data
    sdk.career.get()          -- {board_sales, money, money_earned,
                              --   milestone={index, count, start, goal, thresholds, pending},
                              --   armed, fired, flags, achievements,
                              --   team={name, logo, members, tiers,
                              --     teammates={{joined, name, character, look={kind, id} or nil}}},
                              --   unlocks={{kind, name, new}}, unlock_all, active (activated challenges),
                              --   presets={{key, id, name_text}} (teammate presets),
                              --   graph={id, state} or nil, queue}
    sdk.career.add_sales(amount, reason)  -- board sales (counted as bonus sales), runs the checks
    sdk.career.add_money(amount)          -- money, runs the checks
    sdk.career.arm_action(id)             -- arm a progression action (fires at once if it holds)
    sdk.career.set_flag(name, value)      -- "<mod id>:<name>" unless the career has that flag
    sdk.career.grant_achievement(name)    -- an existing, enabled achievement row, once
    sdk.career.grant_unlock(category, name)  -- an unlock by retail kind 0..12 and name
    sdk.career.grant_reward(id, big)      -- a progression reward row (its unlock + the reward summary)
    sdk.career.skip_start()               -- complete the career start's gate challenges (as owned),
                                          --   so the crossbar shows board sales (also SKATE_CAREER_SKIP_START=1)
    sdk.career.activate_challenge(id)     -- make a challenge available while gating is on
    sdk.career.activate_group(id)         -- every challenge of a progression group
    sdk.career.set_team_name(name)        -- the team's name (16 characters kept; empty allowed)
    sdk.career.set_teammate_name(n, name) -- teammate n's name (1-based)
    sdk.career.set_teammate_preset(n, key)   -- teammate n's look from a preset row (get().presets)
    sdk.career.set_teammate_look(n, profile) -- teammate n's look (a settings/character.json style
                                          --   table; nil clears); removed when the mod stops

Events in `on_event`: `career_paid` (`{challenge, tier, money, sales, rewards}`),
`career_action_fired` (`{action, graph}`), `career_milestone`, `career_teammate`,
`career_tier` (`{tier}`), `career_achievement` (`{achievement}`),
`career_unlock` (`{kind, unlock, reward}`), `career_team_name` (`{team_name}`),
`career_teammate_name` (`{n, teammate_name}`), `career_teammate_look` (`{n, set}`).
Career changes stay when the mod stops (the graphs they fired cannot be undone);
the mod's own flags go. Amounts are capped at 10,000,000 per call. Details:
`docs/hails-additions/43-career.md`.

## Music (`sdk.music`, capability `music` 1)

The music player: the disc soundtrack and players' own music banks (FLAC banks made with
`skate3rust --pack-music-folder`, playlists with `--music-playlist`).

    sdk.music.state()                -- {on, playing={title, artist, custom} or nil, queued, owner, volume}
    sdk.music.playing()              -- the track heard now, or nil
    sdk.music.play_playlist(path)    -- a playlist JSON in the mod's folder replaces the queue
    sdk.music.queue_playlist(path)   --   or is added to it (its banks relative to it; bank and
                                     --   soundtrack entries can be mixed)
    sdk.music.play_soundtrack(track) -- the disc soundtrack: track n (1-based) then the rest, or all shuffled
    sdk.music.stop()                 -- silence
    sdk.music.skip()                 -- the next track
    sdk.music.set_volume(level)      -- 0..2 on the engine's music level; nil: back to the engine's
    sdk.music.set_var(name, value)   -- a variable of the playing disc graph (PathFinder)
    sdk.music.post(event)            -- an event to the playing disc graph
    sdk.music.caption(options)       -- the custom-track caption: {enabled, fade_in, hold, fade_out,
                                     --   left, bottom (percent), title_size, artist_size}; nil: default

Events in `on_event`: `music_track_started` (`{title, artist, custom}`). A mod's queue, volume
and caption changes end when the mod stops (the default soundtrack comes back). Do not share
bank files with copyrighted music. Details: `docs/hails-additions/11-audio.md#music-banks`.

## Multiplayer names

Edit **Your name** in the Multiplayer menu. Names are saved to
`settings/player.json`, transmitted to peers, exposed as `skater.name`, and drawn
above remote players. Supported characters: ASCII letters, digits, spaces,
hyphens, and underscores, up to 16 characters. Empty names display as `Player`.

See `skate.lua` for the complete annotated API. The sample mod in
`examples/game-of-skate` implements game rules using these generic observations.


## Central message

`sdk.ui.central_message(text [, seconds])` shows one line in the HUD's central message,
the retail `hudcentral` movie where a ped's taze shows "ZAP!". `text` is a language id
of that movie (`ID_CHASE_TAZER_1`, shown as its text, "ZAP!") or plain text (1 to 64
characters). It plays the movie's intro and sound, stays `seconds` (default 6, retail's;
up to 60) and then plays the outro; a new line replaces it. Nothing to clean up.

`sdk.ui.chase_alerts(enabled)` shows (`true`, the default) or hides the alerts
over peds that warn or chase the player (retail's yellow and red arrows). A hide
is released when the mod stops.

## Mod menus

`sdk.ui.menu(key, {title=..., items=...})` registers or replaces a mod-owned menu
under **Mods**. Items have `id`, `label`, optional `description`, `enabled`, and
`children`. Children create nested pages; leaves emit an action only to the owner.
Limits are 8 menus/mod, 64 items/menu, 4 levels, with unique item IDs per menu.
Menus support mouse, keyboard, and controller navigation. Remove them explicitly
with `sdk.ui.remove_menu(key)`; they also disappear when the mod stops.

```lua
return {
  on_load=function()
    sdk.ui.menu("challenges", {title="Challenges", items={
      {id="races", label="Races", children={
        {id="sprints", label="Sprints", children={
          {id="downtown", label="Downtown run"}
        }}
      }}
    }})
  end,
  on_event=function(event)
    if event.name=="menu_action" and event.menu=="challenges" then
      -- The mod owns what selecting downtown does.
      sdk.log("Selected "..event.item)
    end
  end
}
```

Callbacks run even while paused, so start/stop/settings actions can take effect
from the menu. Simulation callbacks remain paused. The S.K.A.T.E. example uses
this interface for explicit host-only **Start session** and **Stop session**;
enabling the mod registers its menu without starting a match.

## Native contacts and joint overrides

`sdk.player.read().bailing`, `.mode`, `.state`, and `.category` describe native
player state. `sdk.player.physics()` adds local `ragdoll`, `partial_ragdoll`,
`parts`, `joints`, and `contacts`, along with a simulation `tick` and `dt`.
The convenience methods `contacts()`, `joints()`, and `parts()` return those arrays.
Detailed native physics observations are local; remote mods can publish selected
effects or results through the generic network API.

Contacts are eligible solved native contact reports, capped at 128/frame. They
include both bodies, world point/normal, normal and friction force vectors,
combined static/dynamic friction coefficients, and material tags. Force uses the
native solver's post-solve conversion, not an estimate from player speed; impulse
is that force times simulation dt. These include sustained support forces, not
just new impacts. Use the tick to avoid processing a frame twice, and apply your
own threshold/history policy. Body kind `skater` carries a physical part index;
`external` includes attached proxies and target bodies. No injury is inferred by
the engine.

Joint entries expose their native names, zero-based IDs, parent/child parts,
current angle limits, and override owner. `sdk.player.set_joint(joint.index,
options)` replaces the calling mod's override. Limits are radians, 0.01–pi;
`free_swing=true, free_twist=true` removes angular limits. Also set
`drive_enabled=false` to suppress the child's animation drives. Linear attachment
constraints remain intact: the limb becomes loose, not detached. Another mod
cannot overwrite an owned joint. Overrides are applied to temporary solver input
so native tuning is preserved; `reset_joint(index)`, `reset_joints()`, mod stop,
or world change restores native behavior. Ordinary state transitions and recovery
do not erase an override; the mod decides when to restore it.

```lua
-- Example policy, entirely in Lua: relax the left elbow during a hard bail.
local last_tick
return {on_fixed_update=function()
  local physics=sdk.player.physics()
  if physics.tick==last_tick then return end
  last_tick=physics.tick
  if not sdk.player.read().bailing then return end
  for _, joint in ipairs(physics.joints) do
    if joint.name=="JOINT_LEFT_ARM_FOREARM" then
      for _, contact in ipairs(physics.contacts) do
        local a,b=contact.a,contact.b
        if (a.kind=="skater" and a.index==joint.child)
          or (b.kind=="skater" and b.index==joint.child) then
          local f=contact.force
          -- An illustrative threshold; calibrate it for your mod's behavior.
          if f[1]^2+f[2]^2+f[3]^2 > 5000^2 then
            sdk.player.set_joint(joint.index,
              {free_swing=true,free_twist=true,drive_enabled=false})
          end
        end
      end
    end
  end
end}
```

## Frame-time statistics

`sdk.snapshot.frame` is a read-only view of the engine's frame timing (the same
numbers as the graphics menu's frame-time counter and the `SKATE_FRAME_LOG` log):
`ms` (last frame), `fixed_steps` (physics steps run in that frame), `main_ms` /
`fixed_ms` (CPU time of the engine's main thread / of the physics steps in that frame), `hitch`,
and over the last `window_s` seconds `fps`, `mean_ms`, `median_ms`, `low_1_ms`
(1 % low = 99th-percentile frame time), `low_01_ms` (0.1 % low), `worst_ms` and
`hitches` (frames longer than twice the median of the previous 120). The window
values refresh four times a second. Reading them costs nothing extra; mods cannot
change them. Use them for adaptive effects (drop a costly effect while
`low_1_ms` is high) or a custom performance HUD.

## UI updates during pause

`on_ui_update({dt, paused})` runs every presentation update with a fresh snapshot,
including while the pause menu is open. Use it to refresh menus when network roles
or other observed state changes. It does not advance `sdk.time` or simulation
timers. `sdk.capabilities.menus >= 2` advertises this callback. Keep gameplay in
`on_fixed_update`/`on_update`. Disabled menu actions show their description and do
not invoke callbacks or report a mod failure.

## Lua-defined pause sections

Pass `section` to `sdk.ui.menu` to place the menu in the main pause sidebar:

```lua
sdk.ui.menu("skate", {section="Gamemodes", title="SKATE", items={
  {id="start", label="Start session"}, {id="stop", label="Stop session"}
}})
```

Section names are supplied by Lua and shared by menus with the same name. The
engine imposes no game rules. Omit `section` to use the Mods menu. Menu removal or
mod unload removes its entry; empty sections disappear. Back returns to the
section that opened the menu. Limits: 8 custom sections, 64 section menus total,
32 bytes per section name. `sdk.capabilities.menus >= 3` supports this placement.

## Camera angle and stock shots

The native gameplay camera is Skate 3's stock camera graph. Its **Camera Angle**
setting (pause menu SKATER > Camera angle, `settings/camera.json`) picks the
graph's Low or High branch, which choose different stock shots (`bl_chase` vs
`bl_high_chase`, ...). With `sdk.capabilities.camera >= 4`:

- `sdk.camera.angle()` returns `{selected, active, owner, shot, tuned}`: the
  player's setting, the angle in use, the mod forcing it (or nil), the current
  stock shot and a `shot -> mod` table of tuned shots.
- `sdk.camera.set_angle("low" | "high")` forces an angle; `nil` hands it back to
  the player's setting. One mod at a time; the player's saved choice is untouched.
- `sdk.camera.tune_shot(shot, patch)` replaces stock values of one shot (and of
  every blend tree using it). Keys are the retail attribute names in their units:
  `PositionDistance` (m), `PositionElevation`, `PositionHeading`, `FramingRoll`,
  `FramingYaw`, `FramingPitch` (degrees), `FramingLensLength`,
  `ReferenceBoardOffset`, `SmoothingDirection`, `SmoothingElevation`,
  `SmoothingYaw`, `SmoothingPitch`, `TransitionTime` (s). `nil` restores the stock
  shot. One mod per shot; unknown shots, keys or out-of-range values fail the
  command (use `sdk.commands.request` to get the error back).

Disabling, failing or reloading the mod releases its angle and shot tunings.

## Water look (`sdk.graphics.water_look`, capability `water_look` 1)

Family 33 water (DownTown fountains, the Aletown canal) draws the retail
`water_defaultPS` program plus an optional look layer. The player's values live in
`settings/water-look.json` (missing file = the defaults below; the pause menu
EXTRAS toggle "Better Water" sets `enabled`). With
`enabled = false` the water is the plain retail program.

| Key | Default | Retail |
|---|---|---|
| `enabled` | false (pause menu EXTRAS "Better Water") | false |
| `ripple_scale` | 4 | 1 |
| `reflection_blur` (cube mips) | 3 | 0 |
| `reflection_tint` | {0.82, 1, 1.04} | {1, 1, 1} |
| `third_wave` | true | false |
| `body_calming` | true | false |
| `wave_rate` | 0.75 | 1 |
| `scroll_rate` | 0.5 | 1 |
| `seamless_clock` | true | true (false: the scroll restarts every 10 s) |
| `fountain_colour` (basin body colour, linear rgb 0..1) | {0.018, 0.024, 0.032} | basins keep their own |
| `fountain_reflection` (basin share of its reflection mask, 0..2) | 0.15 | 1 |
| `fountain_ripple` (basin wave tilt gain, 0..8) | 7 | 1 |

`sdk.graphics.water_look(patch)` replaces any of these keys; `nil` restores the
player's look. One mod at a time; unknown keys or out-of-range values fail the
command. Disabling, failing or reloading the mod releases it.

## Local actor suspension and peer cameras

`sdk.player.suspend(true)` freezes the local native simulation, hides its skater,
and removes its multiplayer collision participation. `false` releases only the
calling mod's suspension. Mod retirement, errors, and world changes release its
ownership. Multiple mods may suspend independently. Available with
`sdk.capabilities.player_control >= 1`; skater observations expose `suspended`.

`sdk.camera.watch(peer)` mirrors the connected player's transmitted gameplay
camera transform and vertical FOV (`sdk.capabilities.camera >= 3`). Network
latency still applies. Older peers without camera packets use the body-follow
fallback. `nil` restores the native camera. This does not transfer player input.

`sdk.net.info().host_id` is the authenticated host **player** ID from the roster,
not a Steam ID or socket endpoint. It is `"0"` until the host roster arrives.
Use this player ID with `sdk.net.read`, `sdk.player.skater`, and camera watching.

The Game of SKATE example keeps its turn roster and saved position in an `origin`
record and its compact changing state in `skate`. It waits for every participant
to acknowledge suspension, then resets the active player using
`sdk.player.teleport`. Only that player's post-reset landing/bail counters count.
Late joiners wait for the next match; a missing enabled mod aborts preparation
with an actionable status. Stop/unload returns participants to their pre-match
positions and restores their cameras. Enable the same package version everywhere.

### Native player overlap (capability `player_overlap = 1`)

`sdk.physics.read(key).player_overlapping` reports whether the enabled native
board/skater collision shapes intersect the mod-owned body. Sensors require
intersection; solids include the native solver's 0.025m contact margin. This is
an explicit geometric query independent of collision masks, updated before each
fixed callback. Suspended/attached players report false. No duplicate Rapier
player is simulated. Detect transitions in Lua to count entries rather than
counting every frame of sustained contact. It is a sampled query, not a swept
trigger: use suitably thick sensor volumes for fast-moving actors.

Native foot-support, air-trajectory and camera queries include mod-created
solids, preserving nearest-hit ordering and native map behavior.

### Settled scoring details (`landed_details = 1`)

Player observations include `landed_trick_base` (engine-authored, unlocalized
base label), `landed_spin_degrees` (signed body rotation, excluding board
rotation), `landed_clean` and `landed_sketchy`. These are captured together when
`landing_seq` advances and replicated with that observation. They describe the
last settled trick; compare sequence numbers to distinguish it from old results.
Game rules and label aliases remain in Lua. The existing localized
`landed_trick` remains available for display.

See [GENERAL_API.md](GENERAL_API.md) for native bodies, joint and part overrides, contact lifecycle, input overrides, graph access and acknowledged commands.
