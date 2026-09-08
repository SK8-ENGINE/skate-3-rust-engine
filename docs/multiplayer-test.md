> Latest build: **PLAY-MULTIPLAYER-MENU.bat**, staged in `bin/multiplayer-lobbies`. It opens University offline. Esc > Multiplayer provides public Steam hosting, a paginated lobby browser and joining by stable numeric lobby code. **PLAY-MULTIPLAYER-SMOOTH-2-INSTANCES.bat** uses this same build for the automatic two-player local test. Earlier performance results below are historical.

## Steam lobby test (current build)

Use the menu launcher on each PC with matching assets/build and separate Steam accounts. Choose Host via Steam / Spacewar on one PC. On the other, choose Browse public Steam lobbies, select the matching University row, then Resume. Refresh updates membership counts. You can also type the numeric lobby code and use Join via Steam. Steam is initialized only by Steam multiplayer actions; offline play remains available when Steam is closed. After opening Steam following an error, refresh or retry.

Public discovery is filtered to this game's protocol namespace within AppID 480. Rows show map, occupancy and compatibility; incompatible maps/physics cannot join, and maps are not downloaded or switched automatically. Capacity is ten including the host. There is no friends list or invite UI.

Steam lobby-owner reassignment triggers automatic transport rerouting and a fresh game handshake among the remaining members. Each survivor keeps their own simulated skater and local capture history; remote presentation rebuilds after reconnection. The numeric lobby code stays the same. Test host departure with three players, then verify the two survivors resume and a new player can join. Graceful Leave and an abrupt host exit should both be tested; abrupt failure waits for Steam's membership detection, so migration is not instantaneous. The helper also exits after 15 seconds without its parent heartbeat. Direct UDP tests and legacy SteamID-session codes have no automatic election.

Twenty nongame tests passed, covering bounded discovery messages, Unicode labels, repeated ten-player session migration, local-state preservation, stale departed-host packets, production loopback packets, interpolation and physical contact. Release game/helper builds are staged. No game or Steam process was launched for this verification. Real public discovery, Steam relay connectivity and provider-driven migration still need the multi-PC test above.

> New visual interpolation test: [multiplayer-interpolation.md](multiplayer-interpolation.md). Use its separate launcher for timestamped protocol v3. This page retains the original optimisation results.

# Multiplayer optimisation / next test

Run `PLAY-MULTIPLAYER-2-INSTANCES.bat`. It launches two connected games on **University** using the same ten-player session implementation as online play. Player A hosts; B uses an unavailable outfit identifier and spawns two metres away. Click a window to control that player with raw XInput. `-TwoControllers` assigns slots 0 and 1. Steam is only initialized when choosing Steam multiplayer from the menu.

`PLAY-MULTIPLAYER-10-INSTANCES.bat` launches all ten local game windows. This also tests rendering and physics load on one PC; it is substantially heavier than one game with nine online players. Alternatively pass `-Players 3` through `-Players 10` to the two-instance launcher. Override the map with `-MapPath "C:/path/map.skate"`.

The HUD shows player count, application upload/download in decimal kB/s, worst current direct-link ping, stale packets, missing delta baselines and send errors. Steam sessions additionally show SDK transfer rates, worst link quality estimate, queue time and adapter drops/errors. Host ping is to guests; guest ping is to the host, not the full guest-to-guest path. Sequence gaps are not reported as packet loss because distance throttling intentionally skips source updates.

Test skating into each other, board impacts, tricks, bails, walking, board retrieval and switching window focus. Check the University scenery and remote pose/fallback. Logs are in `logs/multiplayer/<launch-id>/player-*.err.log`, including one `MULTIPLAYER_STATS` sample per second. Opening the menu keeps the online simulation running; replay remains disabled during multiplayer.

## Online setup

On one PC choose **Esc > Multiplayer > Host via Steam / Spacewar**. On each other PC/account enter the same host code in the multiplayer menu and choose Join. Up to nine guests can join. Everyone needs this protocol version and the same map and physical definitions. Visual outfit/rig differences use the default skater. Unknown identifiers never become asset paths. An incompatible visual hierarchy uses the default idle pose instead of foreign bone indices.

The host is an ordinary player, not a dedicated server. All guests connect to that host; it forwards the newest state to the other players. Transport, player IDs, codec, membership, budgets and rendering are independent of Steam. Spacewar/AppID 480 is the optional connection/relay provider. The game has no Steam DLL dependency and neither executable invokes Steam Input. Two processes under one Steam account still require the local launcher for testing.

Guests can reconnect; timed-out actors are removed and slots reused. The current Steam lobby browser and migration flow are described above. Legacy direct sessions require manually starting/joining another host after departure.

## Optimisations

- Version 2 separates physical state and visual anchors into independent datagrams. Full 33-body updates are 825 bytes normally or 1,023 bytes with every position outside the compact range. Thirty compact anchors use 426 bytes. There is no fragment reassembly or lost-fragment amplification.
- Millimetre root-relative positions, four-byte quaternions and half-float velocities. World roots and large relative offsets retain floats. Quantisation adds approximately 0.5 mm per axis plus float precision; quaternion error is bounded by the regression test at 0.006 radians for its samples. This is lossy replication, not deterministic simulation state.
- Per-recipient acknowledged deltas, 64-revision bounded histories and periodic full refreshes. The next delta does not depend on an unacknowledged lost packet.
- Static stock collider geometry stays local. Physical state carries an enabled-part mask and ragdoll flag. Nearby remote bodies join the existing solver with unique reaction indices; distant assemblies are excluded and primitive bounding spheres reject separated pairs before narrow phase. Proxy buffers and resolved remote animation samples are reused/cached.
- Guest capture targets 20 Hz physics and 10 Hz anchors. Host forwarding targets 20/10 Hz within 30 metres, 10/4 Hz within 100 metres and 2/1 Hz beyond. Detached boards are included in distance checks. Unchanged state slows further except nearby physical state. Guest uploads always retain the full rate so two guests near each other do not depend on proximity to the host.
- Physical updates take priority under upload pressure. A latest-state scheduler rotates actors and recipients; stale states are replaced, not queued. State budgets are 180 kB/s per link and 1 MB/s aggregate host; control traffic is additional. Crowded lobbies therefore reduce delivered rates to stay within the host budget.
- Steam state sends use unreliable/no-delay delivery, explicit drop/error counters and queue-pressure feedback. Handshake/control uses unreliable/no-Nagle so Steam can establish the connection. Controls repeat periodically. No reliable snapshot backlog.

Physical collisions still use local ownership with temporary remote proxies, not shared authority or rollback. Up to 50 ms translation prediction, 100 ms visual interpolation and stale-state expiry limit extrapolation; delayed impacts can remain asymmetric. Climbing retains its existing special movement path. This pass does not claim retail Skate 3 online synchronisation or competitive anti-cheat.

## Verification

Release game and relay built and staged using `BUILD-MULTIPLAYER-TEST.bat`, which launches neither. Ten nongame tests passed: existing wire validation, real UDP, quantisation/deltas, truncation, full/mismatched admission, spoof rejection, disconnect/rejoin and a real core-solver contact using the last of nine 33-body remote blocks. The launcher was parsed without execution.

`cargo run --release --target x86_64-pc-windows-msvc -p skate-net --example lobby_audit --locked` runs the production codec/session with ten simulated peers for 30 seconds per scenario, measuring the final 25 seconds. It introduces independent 30–90 ms delay per link, reorder and optional 5% loss. The near scenario changes every body and anchor continuously. It checks all ten memberships, bounded packet sizes, replication freshness, and zero invalid packets/missing acknowledged baselines. It does not run a game, the renderer, Steam or a real WAN.

| Synthetic ten-player scenario | Host up/down kB/s | Guest average up/down kB/s |
|---|---:|---:|
| Moving together, no loss | 1,025.51 / 205.81 | 22.87 / 113.95 |
| Moving together, 5% loss | 1,025.48 / 195.23 | 22.87 / 108.09 |
| Moving 150 m apart, 5% loss | 193.65 / 195.70 | 22.87 / 20.44 |
| Idle together, 5% loss | 189.56 / 36.75 | 4.30 / 20.01 |

These are application datagrams including controls, before UDP/IP/Steam overhead. Crowded host upload is approximately **8.20 Mbps**, guest upload **0.18 Mbps**, guest download **0.86 Mbps** with loss. The host upload is the limiting connection. Choose a host with headroom above those figures. Measured crowded maximum gaps between received body/pose updates were 350/590 ms under 5% loss; this is not a bound for arbitrary internet conditions.

The previous representative serializer used 117.6 kB/s per direction for one peer. The new busy guest upload is about 81% smaller despite the ten-player control overhead. Real game rates depend on motion, distance and relay conditions. In-game feel, ten real clients' performance and Steam connectivity still need the user-run test; no game or Steam process was launched during this pass.
