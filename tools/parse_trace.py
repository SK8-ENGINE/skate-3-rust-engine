#!/usr/bin/env python3
import json
import collections
import sys
from pathlib import Path

def parse_trace(path: Path, gameplay_start_us: int = 15_000_000):
    with path.open(encoding="utf-8") as f:
        data = json.load(f)
    events = data.get("traceEvents", data if isinstance(data, list) else [])
    dur = [
        e
        for e in events
        if isinstance(e, dict)
        and e.get("ph") == "X"
        and "dur" in e
        and e.get("ts", 0) >= gameplay_start_us
    ]
    by_name = collections.defaultdict(lambda: [0, 0])
    by_cat = collections.defaultdict(lambda: [0, 0])
    for e in dur:
        name = e.get("name", "?")
        cat = e.get("cat", "")
        by_name[name][0] += e["dur"]
        by_name[name][1] += 1
        by_cat[cat][0] += e["dur"]
        by_cat[cat][1] += 1
    print(f"=== TOP SYSTEMS (after {gameplay_start_us/1e6:.1f}s) ===")
    for name, (total, count) in sorted(by_name.items(), key=lambda x: -x[1][0])[:50]:
        if total < 1000:
            continue
        print(
            f"{total/1000:9.1f} ms  {count:5d}x  {total/count/1000:6.2f} ms/call  {name}"
        )
    print("\n=== TOP CATEGORIES ===")
    for cat, (total, count) in sorted(by_cat.items(), key=lambda x: -x[1][0])[:20]:
        label = cat or "(none)"
        print(f"{total/1000:9.1f} ms  {count:5d}x  {label}")

def compare_perf(repo: Path):
    for label, name in [("StartPark", "perf.json"), ("University", "perf-university.json")]:
        p = json.loads((repo / name).read_text(encoding="utf-8"))
        print(f"=== {label} ===")
        print(f"  FPS: {p['fps']:.1f}")
        print(
            f"  frame_ms mean/median/p95/max: "
            f"{p['frame_ms_mean']:.2f} / {p['frame_ms_median']:.2f} / "
            f"{p['frame_ms_p95']:.2f} / {p['frame_ms_max']:.2f}"
        )
        print(f"  main_schedule_ms: {p['main_schedule_ms_mean']:.2f}")
        print(
            f"  physics ms/frame: {p['physics_ms_per_frame']:.3f}  "
            f"ticks/frame: {p['physics_ticks_per_frame']:.2f}  "
            f"ms/tick: {p['physics_ms_per_tick']:.3f}"
        )
        print(
            f"  render prepare: {p['render_prepare_ms_mean']:.2f}  "
            f"assets: {p['render_assets_ms_mean']:.2f}  "
            f"views: {p.get('render_views_ms_mean', 0):.2f}  "
            f"queue: {p['render_queue_ms_mean']:.2f}"
        )
        secs = p.get("sections_ms_per_call", {})
        if secs:
            top = sorted(secs.items(), key=lambda x: -x[1])[:5]
            print(f"  sections: {dict(top)}")

if __name__ == "__main__":
    repo = Path(__file__).resolve().parents[1]
    if len(sys.argv) > 1 and sys.argv[1] == "perf":
        compare_perf(repo)
    else:
        trace = repo / (sys.argv[1] if len(sys.argv) > 1 else "trace-university.json")
        parse_trace(trace)
