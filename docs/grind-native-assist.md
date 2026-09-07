# Native grind assist port status

The live game now supports paired-truck50-50 and centre-deck boardslide contacts
(see boardslide-port.md). The new
`skate-core/src/air/trajectory/grind.rs` leaves are not connected to gameplay.
They must not be presented as working magnetic assistance or all-grind support.

Reference: TU3 image SHA256
f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4,
generated PPC in the local Skate3Research/Skate3CustomEngineLayer checkout.
No reference process or gameplay tests were run for this port.

## Ported leaves

- 82D60C80/82D60B98: quadratic trajectory-plane intersection and later-root selection.
- 82D6A398: projected rail trajectory candidate, signed endpoint interval,
  radius plus GrindOffset plane, miss distance, crossing time, fixed60Hz frame,
  rail direction and approach direction.
- 8296EC98/82E09C80: oriented approach angle and fractional-turn fold.
- 82D6A168: height/angle ranking inside the difficulty lock distance and
  distance fallback outside it; removing one winner preserves retry order.
- Admission/displacement portion of82D6A840: relative height, vertical rail,
  perpendicular speed, ledge-side lock scalars, deck-tip allowance and maximum
  trajectory angle. It requires a supplied native surface result; no dummy
  classification or unconditional snap is provided.

These are static source translations, not a claim of verified bitwise parity.
The core crate builds. The user has reserved gameplay testing for themselves.

## Missing live dependencies

1. Preserve the complete indexed world-query result set for82C20728/82C20C08,
   then implement the rail/edge/ledge classification82C21930. The existing
   ground hang-up adapter only produces the fields that its ground caller reads.
2. Complete82D6A840's accepted-target publication and82D6AF58 landing-normal
   construction. Feed the real result through trajectory scoring/second-pass
   selection instead of the current WorldWithoutGrindEdges marker.
3. Connect GrindAirAdjust82D712E0 and its projection, target, and correction
   owners to the physical skeleton/board. The displacement leaf by itself is
   not the complete airborne assist.
4. Port the other contact probes/scorers and state owners. Native manager
   82D875A8 uses separate centre-deck, inverted-deck, truck and tip probes.
   State400 boardslide,40150-50,402 tipslide,4035-0,404 backslash and405
   darkslide are distinct physics states. Naming variants requires the actual
   chromosome producer as well as those state IDs.

## Proposed read-only reference trace

A reference run was authorized and launched, but its controller backend did
not expose the physical controller. The user requested continuation from code.
No successful reference grind capture was obtained and no synthetic gameplay
input was sent.

Capture query inputs and indexed hit validity/fraction/position/normal/surface
at82C20728 and82C20C08, the classification at82C21930, ranked candidates and
accepted/rejected attempts at82D6A168/82D6A840, and GrindAirAdjust output at
82D712E0. Include state/category, mode, simulation tick, spline ID, board frame
and controller inputs so these observations can be matched to their callers.
Keep the trace local and do not modify the reference binaries or database.
