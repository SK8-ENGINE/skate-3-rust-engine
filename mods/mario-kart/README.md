# Mario Kart vehicle mod
Enable this mod in Mods, resume, then press F10 to spawn the kart in front of you.
E (controller Y) enters/exits within 4 metres (slow below 3 m/s to exit). WASD drives; Space brakes;
left Shift is the handbrake; R rights/resets the kart. F10 replaces the kart.

Controller: RT/LT accelerate/reverse, left stick steers, A brakes, B handbrakes.

The GLB is prepared from your supplied ZIP with tools/prepare_mario_kart.py and is
kept out of Git. To rebuild it: python tools/prepare_mario_kart.py PATH_TO_ZIP mods/mario-kart
The script requires NumPy. No third-party model is downloaded or bundled in source commits.

Edit vehicle.json for dimensions, wheels, suspension, seat, exit and camera offsets.
Animation slots are intentionally null. With no animation the native skater is hidden
while driving. Supply a compatible vehicle animation file and name its clips in the
enter/exit/drive/idle/reverse/brake/steer_left/steer_right slots to display the rider.
See docs/vehicle-sdk.md for the animation format and lifecycle details.
