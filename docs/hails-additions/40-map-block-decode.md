# 40. Map block decode: exact-size buffers (upstream PR #66)

Part of PR #52 (an optimisation item the user added 2026-10-21, USER-ASKS 180). The change is JohnUzoka's upstream PR
[#66](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/66), taken unchanged with their authorship (3 commits);
#52 only adds a regression check on top. #66 has since grown to six commits (its first three rebased, the second
revised), so merging it first now conflicts with #52 in five files: #52 waits for #66 to merge, then merges main in
(user 2026-10-25: do not pull its new commits; checked with `git merge-tree` against `pull/66/head` 7d707e8c).

## Problem

`.skate` packages store their blocks compressed (zlib or zstd). `StoredBlock::decode` seeded the output with
`Vec::with_capacity(expected.min(8 MiB))` and filled it with `read_to_end`. A `Take` over a decoder gives no size
hint, so std grows the buffer by doubling: 8, 16, 32, 64, 128, 256 MiB for DownTown's 140.3 MiB vertex block, the old
buffer alive during each copy. On memory-constrained targets (JohnUzoka's Switch port, 3 GiB heap) that aborted.

## Root cause and evidence (checked at the source 2026-10-21)

1. The doubling is on upstream/main, our main and hails-additions alike (`StoredBlock::decode`).
2. #66's first commit reserved the exact size in `decode`, but the v9+ geometry merge then did
   `bytes = stored(vertices); bytes.extend(stored(indices)); bytes.extend(stored(collision))`, so the first `extend`
   regrew the exact vertex buffer to about twice its size: the peak only moved. We said so on #66; their follow-up
   commits fix it.
3. An up-front reservation of a block's declared size lets a tiny bad file force a huge reservation before failing;
   their last commit rejects such sizes first.

## Change (JohnUzoka, `crates/skate-data/src/skate_map.rs`, `skate_map/storage_v15.rs`, `tests/skate_map.rs`)

- `f41aa4a7` "Maps: decode stored blocks at their exact size; stop doubling": exact reservation in `decode`.
- `41f23894` "Maps: decode the geometry blocks into one buffer reserved once": `decode_into` appends one block to a
  caller's buffer, `exact_buffer` = `try_reserve_exact`, `decode_joined` reserves the three geometry blocks' sum once;
  `read_declared` reads exactly the declared size and then requires the stream to end, so an overlong block never
  grows the buffer; the v15 index and collision transforms reserve their exact output after a payload-length check.
- `468e7c29` "Maps: reject block sizes the stored bytes cannot decode to": `check_size` rejects an impossible declared
  size before reserving (stored = its length, deflate 1032:1, zstd 32768:1, SKATE15 = 4 x the bound of its payload);
  the RGBA row filter checks its row bytes before allocating.

Ours (on top): the ignored test `installed_map_geometry_fingerprints` (below). When the commits were taken, both
branches had a `#[cfg(test)] mod tests` at the end of `skate_map.rs`; their tests and `stored()` helper were merged into
the existing module (otherwise E0428).

## Verification

- Tests (`skate-data`, theirs): `blocks_decode_into_exactly_their_declared_size`,
  `a_block_that_decodes_longer_fails_without_growing_its_buffer`, `geometry_blocks_decode_into_one_buffer_reserved_once`,
  `index_and_collision_transforms_reserve_their_exact_size`, `filtered_rows_are_checked_before_the_image_is_allocated`,
  `a_tiny_block_declaring_1_gib_fails_without_reserving_it`, `rejects_blocks_declaring_more_than_their_bytes_can_hold`;
  the existing version / v15 / geometry tests stay green (`cargo test -p skate-data --lib --tests`, 2026-10-21).
- Outcome identical (regression-check 3b): `installed_map_geometry_fingerprints` printed the same vertex / index /
  collision fingerprint for all 17 installed maps (10 disc, 7 DLC) with the old decoder (`1f2adaae`) and with #66's
  three commits (2026-10-21 16:05).
- Memory on PC (Windows, the fingerprint test loading every map, measured with our earlier version of the same idea):
  peak working set 1,670 MiB and peak committed memory 1,893 MiB with either decoder. The decode-stage doubling is
  real, but on PC it is not the load's highest point: that comes later, while the decoded geometry bytes and the
  parsed vertex structures are both alive. A target whose allocator fails on the doubled reservation itself (the
  Switch heap) is where this helps; not measured here.

## Open

- The later peak (geometry bytes plus parsed vertices) would need the vertices decoded in place or streamed.
