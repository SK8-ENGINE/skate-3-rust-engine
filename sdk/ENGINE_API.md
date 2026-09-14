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

`sdk.player.teleport({position={x,y,z}, heading=radians, velocity={x,y,z}})`
uses native player travel. Heading defaults to zero; velocity is optional. This
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

## Multiplayer names

Edit **Your name** in the Multiplayer menu. Names are saved to
`settings/player.json`, transmitted to peers, exposed as `skater.name`, and drawn
above remote players. Supported characters: ASCII letters, digits, spaces,
hyphens, and underscores, up to 16 characters. Empty names display as `Player`.

See `skate.lua` for the complete annotated API. The sample mod in
`examples/game-of-skate` implements game rules using these generic observations.


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
