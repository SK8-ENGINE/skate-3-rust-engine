# Skyline DRIVE + Audio

This package updates the **latest uploaded `skylineworking.zip`**, not an earlier car rebuild. It retains the actual `skyline.glb`, all 16 sound files, the sound-bank metadata, and the existing audio controller. The current engine, gearbox, tire law, clutch, differential, brakes, chassis and suspension parameters are retained.

**Requires Driving API extension 1 in the rebuilt game executable.** Merely replacing the Lua API file is insufficient. On an older executable this mod falls back to the old fixed follow view and shows a rebuild warning; the new camera and dashboard need the native extension.

## Install

Close the game. Apply the separate `Skate_Driving_API_Update` source package, rebuild with your normal `BUILD.bat`, and launch the rebuilt game. Back up the old mod OUTSIDE the scanned mods directory, then replace it with this entire folder. Keep only one enabled package with ID `examples.skyline`.

The included original Skyline model is selected directly. There is no fallback-model setting to enable. The unused fallback asset is retained solely to preserve the uploaded package. Existing saved tire/audio settings continue to use the same keys; the new diagnostics setting deliberately defaults to off.

Press F10 for a fresh car and E or controller Y to enter. The Mods menu identifies the package as `Skyline DRIVE + Audio`, version 4.2.0. The clean speedometer appears while occupied.

## Controls

| Action | Controller | Keyboard |
|---|---|---|
| Dynamic chase / hood view | **X** | **C** |
| Enter / exit | Y | E |
| Throttle / service brake | RT / LT | W / S |
| Steer | Left stick | A / D |
| Handbrake | B | Shift |
| Shift up / down | RB / LB | **X** / Z |
| Neutral | — | N |
| Reset occupied car | Right-stick click | R |
| Spawn a fresh car | — | F10 |
| Toggle car diagnostics | — | H |

Keyboard X still shifts up; controller X switches views. Camera switching is edge-triggered, so holding X does not flicker between cameras.

## Camera

The chase camera is a persistent native render-rate rig, not a fixed Lua offset. A critically damped spring follows relative offsets; heading lag, bounded acceleration lag, velocity look-ahead and modest speed-dependent framing make it dynamic. The camera pulls back up to another 1.8 m and gains up to 8 degrees of vertical FOV. At constant speed, position lag does not grow indefinitely with speed.

Camera and owned car visuals use the same interpolated native body sample. This changes rendering only; the physics body is not moved. Hood view is rigidly attached above the uploaded model's hood, inheriting actual chassis pitch and roll without synthetic shake. The hood height is adjustable to account for later visual-model edits.

Chase collision avoidance uses five map-only rays and a safety margin. It snaps inward when obstructed and eases back outward. **This is not a swept sphere**, does not check movable props or other cars, and has not been verified in the game. Tight corners/thin geometry remain an in-game test case. Switching away, exiting, unloading, or changing worlds releases the camera and restores the near clipping plane; the base game's camera supplies its normal FOV again.

Settings: Chase distance, Chase spring frequency, Chase vertical FOV, Hood camera height. Lower spring frequency produces more lag; higher is tighter. Camera effects never modify tire forces, steering commands, yaw, or physical body pose.

## Dashboard and diagnostics

The lower-right canvas shows road-plane chassis speed, selected R/N/1–6 gear, engine RPM, a rev bar, handbrake status, and the current view. It uses keyed, retained rectangles/text, not dozens of temporary objects each tick. Content refreshes at 20 Hz; the native canvas remains visible between updates and fits the viewport. MPH, HUD scale and visibility are settings.

The speed is the magnitude of chassis velocity in the body's road plane, not driven-wheel speed; a stationary burnout does not read as vehicle speed. Gear and RPM are actual simulation values, not decorative estimates.

**Show car diagnostics (H)** defaults to off. H overrides it for the current session; the menu setting is persistent. The dashboard remains usable with diagnostics off. This controls the car's top-left diagnostic text, not the engine's independent FPS/profiling displays. The native canvas is hidden behind paused/replay/customizer views and removed when leaving the car.

## Steering: a driver-input change, not a stability controller

Small analog stick movements become gentler as speed rises from 54 to 198 km/h. This is a nonlinear mapping between a short-travel thumbstick and the steering actuator. It does not inspect sideslip, target yaw, tire saturation, or drift state. Full stick still requests the original +/-40-degree virtual rack at any speed; Ackermann inner/outer angles and the original rack-rate limit are retained. Opposite lock is not clipped away during a slide.

Set **High-speed stick precision = 0** to restore the old analog curve exactly. A/D ramps the driver's requested input over a finite interval instead of instantly switching from centre to full demand. Releasing the keys requests centre; this is not tire-driven self-aligning steering.

Full steering at high speed can still saturate tires and spin the car. The mapping improves fine input resolution; it does not make an impossible high-speed turn possible or add a hidden corrective force.

## Downforce

The uploaded mod included drag, but no downforce. This version retains that drag and adds optional, explicitly modeled front/rear aerodynamic surface loads:

`F = 0.5 * rho * V_forward^2 * (Cl * A)`

Defaults are air density 1.225 kg/m^3, front downforce coefficient-area 0.08 m^2 and rear 0.12 m^2. These are **modest tuning assumptions, NOT measured R34 coefficients**. At ideal straight, level motion the total is approximately 94.5 N at 100 km/h and 378.1 N at 200 km/h. This is only about 2.75% of the modeled 1400 kg car's weight at 200 km/h, not a huge magnetic force keeping it upright.

Each surface force is perpendicular to its local airflow and acts at a declared front or rear point, so both net force and pitch moment enter the existing chassis/contact solver. Direction follows body orientation rather than always pulling toward world-down. It is not conditioned on wheel contact, and no tire-friction multiplier, automatic countersteering, grip-recovery mode, artificial upright torque, or chassis velocity overwrite is added. Positive coefficient-area means downforce; negative means lift. Turning **Aerodynamic surface loads** off disables these added loads but retains the pre-existing drag.

This remains a reduced model: coefficients have not been fitted to wind-tunnel data, there is no full angle-of-attack/ground-effect map, and existing drag is not a newly calibrated 3D aero model. The implementation is not a claim of stock Skyline fidelity.

## Evidence and limitations

The delivery's actual Lua 5.4 and updated API wrapper passed:

- 94 baseline numerical scenarios with optional precision/aero disabled, including 10,000 randomized passive tire/brake contact checks.
- The 94-scenario suite with new steering/aero defaults, at 30/60/120 Hz and the explicitly labeled friction values (unqualified cases use 1.3, not the manifest's 1.25 default).
- 19 focused driving-extension scenarios covering inputs, HUD data and bounds, audio continuity, lifecycle cleanup, steering authority, aerodynamic force/moment equations, and numerical isolation of presentation from physics.

Maximum measured callback usage was 83,000/100,000 Lua instructions and 50/128 queued commands in those suites. The source patch contains reproducible test code and native Rust unit tests.

**Rust compilation and in-game camera/HUD/audio playback have not been performed for this update.** The provided source archive has no workspace-root Cargo.toml/Cargo.lock, and the build environment used for this delivery has no Rust compiler. The numerical host is not Rapier and does not render the GLB/UI or native camera collision rays. Use the native smoke-test checklist in the API update after rebuilding.
