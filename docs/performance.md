# Large map performance

The September 6, 2026 performance pass preserves the full render geometry and
collision data. It does not add LODs or simplify physics.

Changes:

- A bounds hierarchy selects collision clusters instead of scanning all clusters.
  Results retain their original order, including equal-distance hits.
- Conservative triangle bounds reject impossible line and primitive contacts
  before the expensive narrowphase. Predictive separation and fatness are included.
- Byte-identical textures and equivalent rendered materials share resources and
  batches. Texture color space and all currently rendered material fields remain
  part of their identity. Every source render triangle is retained.
- Render meshes and textures release their CPU copies after GPU upload. The
  decoded map package is released after physics and rendering have consumed it.
- The normal development build now optimizes project code at level 3, matching
  the existing dependency optimization level.

## Repeatable measurement

From the project directory in PowerShell:

```powershell
./Build.ps1
$env:SKATE_PERF_REPORT = 'logs/performance.json'
./bin/skate-game.exe --assets assets --map maps/Skate_2_New_San_Vanelona.skate
Remove-Item Env:SKATE_PERF_REPORT
```

The benchmark runs continuously regardless of window focus, waits 10 seconds
for warmup, records roughly 15 seconds, writes JSON, and exits. Leave the player
stationary for spawn comparisons. The environment variable enables instrumentation;
normal launches do not collect reports or change their window-focus behavior.
Frame intervals include renderer synchronization. Main schedule and render timings
overlap and must not be added together. Detailed physics section timings include
warmup and nested scopes; these must not be summed either.

Local San Vanelona measurements at 1280 x 800, stationary spawn, RTX 5090:

| Metric | Before | After |
| --- | ---: | ---: |
| Average FPS | 43.6 | 50.7 |
| Main schedule per frame | 10.56 ms | 2.39 ms |
| Physics per tick | 6.44 ms | 0.74 ms |
| Frame interval p95 | 128.1 ms | 94.5 ms |

Sources are local ignored reports `logs/perf-baseline-continuous.json` and
`logs/perf-render-detail.json`. Repeated optimized runs averaged 49–52 FPS.
Process working set fell from about 10 GB to roughly 1–2.3 GB after loading.
These are spawn measurements, not a reproduction of the reported 10 FPS camera
position. Render preparation remains the largest measured cost, and frame spikes
remain. Low total CPU/GPU utilization does not exclude a CPU render bottleneck.

Spatial render splitting was also tested, but increased entity/batch overhead and
reduced FPS on this map, so it is not included.

Validation includes the full skate-core suite and game suite (43 passed, 21 existing
ignored tests), differential collision queries against the unfiltered implementation
across 420 varied poses, material/texture identity checks, and exact render triangle
retention tests. Startup capture checks rendering/integration, not retail parity.
