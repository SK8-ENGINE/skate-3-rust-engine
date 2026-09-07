# Live grind integration

The default world now supplies seven straight grind paths shared with rail/coping collision geometry. This pass connects the paired-truck 50-50 acquisition path to the production state selector, board forces, skeleton update, stock grind animations and physical output. Imported map grind descriptors are not wired into this owner yet.

## Source and implementation

TU3 image: default_82000000_011B0000.bin, SHA256 f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4. Analysis used the local generated PPC and the copied IDA database; the reference game was not launched for this implementation.

- 82C1FDC0 and 82D89150: actual truck probe rectangles, paired same-primitive contacts, depth and balance rejection. No nearest-distance snap or positional teleport.
- 82D886B8: air and ordinary level-ground admission. The host currently covers straight level rails; sloped approach classification, adjacency arbitration, tipslides, boardslides and darkslides still need their own live adapters.
- 82D41D70 / 82D3FA18 / 82D3FD88: 50-50 lateral pin and friction, using the stock PinVsSlope and FrictionVsTime curves. Authored static rail support uses the upward normal and default surface multiplier.
- 82D41E38 / 82D40890: rail-aligned target blended through the existing native matrix interpolation and angular-only board drive. User-driven grind twist forces and the remaining grind-manager transition scoring are not complete.
- 82D73AB0: straight static 50-50 pop branch, stock physics-mode launch speeds, lateral nudge and launch velocity publication.
- 82D3F430: disables the grind hook and restores wheel drag on exit. The original selector retains Nonspecific701 briefly on contact loss before choosing air/ground from actual observations; the host now supports that ordinary continuation.
- 82BAF208, 82BB0A30, 82BB0D40 and 82BB10F8: selected grind intent, stock twist endpoints and retained fade, and grind crouch. Physical twist uses the native parts10/6 direction relative to Processed352 and Reckoning up. Canonical 50-50 names select the existing B_GRIND5050/B_BF_GRIND5050 animation trees; full chromosome naming/scorable identifiers are not ported.
- 82BA8238: DistToEdge reads the existing OffBoard116 output. This resolves the mounting graph condition encountered in the launched build.

## Visual check

Build with Build.ps1; launch bin/skate-game.exe --assets assets (no map argument). The low straight rail is at x=-7, z=7..15, 0.45m above the lower floor; the higher rail is at x=4.5, z=8..16, 0.70m above it. Approach along the rail and land both trucks on its top. The halfpipe coping and three starting-platform edges also have authored paths.

Build completed. No automated tests or controller-driven gameplay tests were run. Visual behavior, catch consistency, pop timing, naming and solver stability while grinding require the user's playtest; this is not a claim of complete Skate 3 grind parity.
