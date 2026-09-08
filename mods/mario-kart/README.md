# Mario Kart vehicle mod
Enable this mod in Mods, resume, then press F10 to spawn the kart in front of you.
E (controller Y) enters/exits within 4 metres (slow below 3 m/s to exit). WASD drives; Space brakes;
left Shift is the handbrake; R or right-stick click rights/resets the kart. The controller bind can be changed to left-stick click in the mod settings. F10 replaces the kart.

Controller: RT/LT accelerate/reverse, left stick steers, A brakes, B handbrakes.
The enabled mod shows keyboard and controller controls on screen, including the current reset bind. Toggle Show driving HUD in its settings to hide/show them, even before spawning.

The GLB is prepared from your supplied ZIP with tools/prepare_mario_kart.py and is
kept out of Git. To rebuild it: python tools/prepare_mario_kart.py PATH_TO_ZIP mods/mario-kart
The script requires NumPy. No third-party model is downloaded or bundled in source commits.

Edit vehicle.json for dimensions, wheels, suspension, seat, exit and camera offsets.
The local rider.json contains fitted entry, exit, seated and left/right steering poses.
The host blends steering with stick input and eases between vanilla and vehicle poses
using the native local-transform interpolation routine. The skateboard is hidden while
seated and restored during the exit hand-off. Camera changes are eased too.

Rebuild rider.json with Blender and tools/export_kart_rider.py, passing --enter and
--exit fitted .blend previews, --bone-names a JSON list from the stock bank hierarchy,
--reference the stock skater.glb and --output the local
rider.json path. Character/model assets and exported animation data stay out of Git.
See docs/vehicle-sdk.md for the animation format and lifecycle details.
