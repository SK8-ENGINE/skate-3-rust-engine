# 33. Rolling on grass and dirt: no stand-in grain, retail routing pinned for every surface

Branch `fix/grass-rolling` (from `hails-additions` d7e5d09; the touched code files are identical on `main` b3c9679); the fix is also on `world/living-world` (2026-10-09).
Status: **the grain bed's stand-in grains are replaced by retail's routing and every surface tag is pinned by a parity
test. The grass report itself is not a bug: retail plays the same SpiderCracks layer (Class_rolling layer 5) on that
grass. Retail code and retail collision data both give seam pattern 1 there, and the user confirmed it in the recomp
(2026-10-08 17:39, recording kept locally): "YEP you were right. It does play concrete there, I guess the texture is
kind of rock like.."**

Labels: **[verified]** = shown by code or data with the location given; **[unverified]** = inference or not measured.

## Problem

The user skated over grass / dirt near Ghetto Spot in Industrial on the `hails-additions` build and heard a concrete
rolling sound. First verdict (verbatim): "IT DOES NOT MAKE CRACK CLACKS IN THE GRASS IN RETAIL." After testing in
the recomp (verbatim): "YEP you were right. It does play concrete there, I guess the texture is kind of rock like.."

`crates/skate-game/src/game_audio/grain_bed.rs` (`grain_for`) gave the rolling surfaces that retail plays no grain on a
stand-in grain: tags 8, 10, 70 (rolling surfaces 8, 7) `concrete_aggregate_hard`, tags 37, 67..69 (surfaces 13, 10, 12)
`metal_smooth_hard`, tag 90 (surface 0) `wood_ramp_hard`.

## Findings

1. **[verified] The stand-in grain did not play in the user's run.** Stderr log
   `.local/versions/hails-additions/logs/game-20261008-161024.stderr.log`: `native rolling layers true` (22:10:30.90), so
   the bed takes its binds from the owner routing (`player::rolling`, port of `sub_824C5CA8`); `grain_for` was only the
   fallback when the Class_rolling banks are missing. On the grass ride (state log
   `.local/audio-state-logs/state_20261008_161024.tsv`, last segment, ms 23.8 to 26.7 s, 169 rows with all wheels on tag 8
   around (-573, 10, -308), 8 to 12 m/s) the log has no `grain bind` between 22:21:41.906 and the jump at 22:21:45.086.
   That "tag 8 is grass" rests on the user's report of where they rode plus the tag at that spot **[unverified: no
   material name table checked]**.
2. **[verified] What our engine posted on the grass** (same log, every non-redeliver audio line 22:21:42 to 22:21:44):
   `Class_rolling RollingSurface(1)` selector 2 / surface 8 (41.906), `Class_rolling RollingLayer(5)` = layer 5
   (42.005, released 45.086), `Class_wheels_skid` x3, `SenseOfSpeed_rattle` x2, `Class_Squeaks`, `cloth_trick`; the held
   layers 0 / 3 run all the time. No `Class_Seams` hit is possible there: the install's seam pattern record 1 has mode 0
   (`assets/private/audio/audio_manifest.json` `player_tuning.seam_wobbles[1]`), and `seams.rs` `trigger` returns on mode
   0. Which of these voices were audible and how loud is **[unverified]**: the session logs posts, not voice gains.
3. **[verified] Why layer 5 posts there.** The state log's `seam0` / `seam3` are 1 on all 169 grass rows (0 on the tag 4
   concrete rows). Our `rolling.rs` (`Rolling::process`, the `pattern == 1` block) posts layer 5 on wheel 0's pattern 1,
   as retail `sub_824C9F68` (`.local/audio-re/board-layers/f_824C9F68.txt`: `lwz r28,636(r11)`, `cmpwi cr6,r28,1`, post
   `sub_824C4C18(r29, 5, 3)`; no surface test). The pattern comes from the wheel-line hit tag: retail loop at
   `loc_82C07A18` (skate3recomp generated `skate3_recomp.75.cpp`) stores `rlwinm r6,r7,20,28,31` = (tag >> 12) & 0xF,
   exactly our `board_ground.rs` `seam_patterns[i] = (hit.surface_tag >> 12) & 0xF`. That retail's audio state `+636`
   is this wheel-line field: `sub_824B0DA8` copies `+452` of its source into `+636`
   (`f_824B0DA8.txt` lines 344..358) **[the link from the wheel-line struct to that source is unverified]**.
   Pattern id 1 = the vault's spidercrack record: it is the only record with a wobble other than {1, 1} (0.8 / 0.6,
   30..80 ms), which grain-player-spec section 2.7 names spidercrack **[verified in the install data; the name itself
   comes from the spec]**.
4. **[verified] The SpiderCracks program does not silence surface 8 in our evaluator.** `sub_824CA038`
   (`f_824CA038.txt`, `bl 0x824c82a8` then `stw r11,28(r10)`) writes the truck's rolling surface into w6 every update, as
   ours does. The data-gated render `skate-audio` `rolling_banks::rolling_layers_play_their_retail_banks` (run
   2026-10-08 against the extracted banks and the install MixMap) gives PatchBank_SpiderCracks the same per-voice gains
   on surface 8 (tag 8) as on surface 3 (tag 1): max 0.226 / 0.277 / 0.355 at 8 / 20 / 35 km/h. So with these inputs
   our port is audible on grass, against the user's verdict.
5. **[verified, but NOT grass at Ghetto Spot] Retail trace.** In `.local/recomp/sessions/localtest_20261003_102703`
   (scripted run at the University spawn, around (412, 136, -712)), SEAMPAT lines show wheel 0 material 7 with pattern 1,
   and POST lines from `824C9FE8` (layer 5) at 61418.4 and 78906.4 ms, at 0.21 m/s at the first post. That session has no
   GAIN / SEND lines, so audibility is unknown, and that material 7 there is grass is **[unverified]**. It does not show
   what retail does at Ghetto Spot.
6. **[verified] The stand-in table was not retail for the bed's own routing.** `sub_824C5CA8`
   (`f_824C5CA8.txt`: `addi r11,r27,-7` switch; `+1488` selectors 7 -> 1, 8 -> 2, 10 -> 10, 11 -> 12, 12 -> 11,
   13 -> 9) binds a grain (`sub_824C8878`) only on kind-1 surfaces; on 7, 8, 10..13 it posts the Class_rolling patch and
   starts no grain. Surface 0 goes to `sub_824C8370`'s `default` key (`f_824C8370.txt`: r31 = 0xD7EDBD362D7D2152, the
   jump table skipped for surface - 1 > 8).

## What would settle the grass question (settled 2026-10-08)

Settled without this run: retail parity, see Status (retail plays the same SpiderCracks layer there, confirmed in the
recomp). Kept as the method for a similar report.

A short scripted recomp run (no user session): trace-all hooks SEAMPAT + POST + PLAY + GAIN + SEND, teleport onto the
Ghetto Spot grass and roll straight about 3 s at 15 to 25 km/h. It shows (a) retail's `+636` there (pattern 1 or not; if
not, our collision tag at that spot differs from retail's), (b) whether `824C9FE8` posts layer 5, and (c) the
SpiderCracks voice gains (if posted but silent, the MixMap SkateBoard level(10) or the program differs from our port).
Also needed: the retail coordinates of that spot (our (-573, 10, -308) has not been mapped to the recomp's frame).

## Change

- `crates/skate-audio/src/player/rolling.rs`: `retail_surface(material)`, the vault `Sk8::AudioSurfaceMap` word +4
  (`sub_82494CD8`, decoded in grain-player-spec section 1.4) as the default when the install has no table, and
  `surface_for(tuning, material)` (143 → 3, the install's table first, a mod's audio tuning can replace it).
- `crates/skate-game/src/game_audio/grain_bed.rs`: `grain_for` and `Bed::member` removed. Both routings now bind through
  one function, `bound_for(surface, soft)`: `rolling::grain_surface` decides grain or no grain, `rolling::member` the
  member and tuning, soft members the install lacks fall back to the hard one. The fallback route reads the tag's
  surface with `surface_of_tag` (install table, else the retail default). No grain on grass / dirt / the other
  Class_rolling surfaces in either route.

Moddability: the surface of a tag comes from the install's AudioSurfaceMap (`audio_export` player tuning), so a mod's
audio tuning re-routes a tag the same way for both routes (test below); the retail values are only the default. Nothing
is cached, so disabling the mod restores the retail routing on the next frame.

## Verification

- `skate-audio` `player::rolling::tests::every_surface_tag_routes_as_retail` (plain): every tag 0..127, hard and soft
  wheels, through `Rolling::process`: tag → material (tag - 1, 0 → 143) → rolling surface (`sub_824C82A8` +
  `sub_82494CD8`) → grain surfaces 1..6, 9: exactly one grain bind of `sub_824C8370`'s member and no Class_rolling
  patch; Class_rolling surfaces 7, 8, 10, 12, 13: no grain, one `RollingSurface` post with the `+1488` selector in w4
  and the surface in w6 (`sub_824C4C18`), and the rattle gate off; plus concrete → grass stops the grain and posts
  selector 2 / surface 8. Surface 0 (tag 90) is listed as **UNVERIFIED** (the recording the `default` key binds is not
  traced; we bind asphalt_rough_hard with the `default` tuning). Passes; `skate-audio` lib 260 passed, 14 ignored.
- `skate-game` `grain_bed::tests::rolling_grains_follow_the_retail_routing` (plain): the bed's own routing binds the
  same members, none on tags 8, 10, 70, 67, 68, 69, 37, soft and missing-member fallbacks, and an install table that
  re-routes tag 8.
- `grain_bed::tests::grain_for_matches_the_vault_surface_map_and_the_bed_loads` (ignored, private install data):
  every tag against the install's AudioSurfaceMap, and the bed's binding per surface (none on 7, 8, 10..13); run
  2026-10-08 against the user's install: passes (the decoded map equals the install's for all 128 tags).
- `skate-game` lib tests in the touched module pass (`rolling_grains_follow_the_retail_routing`).

## Open questions

- On that grass our board accelerated from 8 to 12 m/s; above 35 km/h the SenseOfSpeed rocket (`x_jet_rolling`) starts
  (`grain-player-spec` section 2.8). Whether retail grass slows the board there is a physics question, not covered here.
- Surface 0 (tag 90): which recording `sub_824C8878` plays with the `default` key (listed as unverified in the test).
