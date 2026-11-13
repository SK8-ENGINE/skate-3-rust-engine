"""Shared reader for skate3recomp research traces (`trace.tsv` from the hooks in src/research/*).

Lines are `KIND <tab> <ms> <tab> fields…`, all on one time base. Special kinds:
- MARK <ms> <label>: script steps, e.g. "at <stop>", "listen <stop>", "done";
- CLOCK <ms> <unix ms>: aligns trace time with wall clock (screenshot names);
- CAPTURE <ms> <frames>: aligns trace time with the float32 stereo capture (`trace.f32`).

    from trace import Trace
    t = Trace('<session dir>/trace.tsv')
    for stop, lines in t.by_stop('WATCH'):          # lines are (ms, fields)
        ...
    t.shot_for(ms)    # nearest screenshot path
    t.capture(a_ms, b_ms)  # numpy (n, 2) float32 window
"""
from __future__ import annotations

from bisect import bisect_left
from collections import defaultdict
from pathlib import Path
from typing import Iterator


# Fixed field counts (after KIND and ms) for kinds whose hooks write a fixed layout; `Trace.malformed()` checks
# them. Hook documentation: the header comment above each hook in skate3recomp src/research/hooks_*.cpp.
FIELD_COUNTS = {
    'TREAT': 8,     # +236 +240 +260 +224 +332 +200 object local   (Class_Treatment, sub_824DD6F0)
    'SEAMPAT': 11,  # +636 +648 +620 +208 frame_ms w0 w1 w2 w3 ("x y z") object local   (Class_Seams, sub_824C14C8)
    'SEAMHIT': 5,   # wheel single transition object local   (seam hit, sub_824C1DF8)
    # Category audiox (2026-10-03; categories audio,audiox,dsp). Space-separated groups count as one field.
    'GRECX': 7,     # owner w332(hex) slip232 rev690 counter1516 cam("x y z") view("x y z")   (sub_824C6BD8, local player)
    'FIRSTHIT': 6,  # B b16(u8) f24 bail676 end677 why(1 byte 2 float 4 bail/end 8 heartbeat)   (GREC hook, B = *(*(0x83083C38)+0x2FCB4))
    'SKID': 7,      # owner holder handle w0..w17(18 ints, or "-" on release) counter1516 slip232 rev690   (sub_824C7A20)
    'EMITSLOT': 11, # object state info0(patch) info4(positional) info8(level) info12 info16 mixkey handle w0..w8 out0..out9(hex)   (sub_824DCF08)
    # Bail body impacts (2026-10-03, audiox; local player, only while bailing). Summary: tools/bail_impacts.py.
    'BAILLOCAL': 5, # character object1808 X how(1 vtable checked, 2 not) entry_index   (sub_827A1B78)
    'BAILSTEP': 10, # X step_ms dt5220 bail end contacts sc4036 sc4040 cfg164 how   (sub_82BD60C8, after its BAILREGs)
    'BAILCAND': 7,  # src(entry|pass) lr a b flag c d: lookup diagnostics (entry: character entry bit31 object X; pass: SC X 0 0 X)
    'COLLPOST': 16, # caller object state grec_owner local72 matA matB tierA tierB soundA soundB b40 b41 b42 pos("x y z") chain   (sub_82486EF0)
    'BANDQ': 7,     # caller material impact flag tier low high: impact-band queries sub_82497088 (2026-10-03, audiox)
    'LOCALTEST': 6, # hook object byte16 byte28 old_word accepted: local-test decisions per object (2026-10-03, audio)
    'BAILREG': 12,  # X region part impact |dv.n| v_old.n v_new.n |dv| normal("x y z") mass slide tag(hex)   (sub_82BD60C8)
    # World audio gaps G1 / G2 (2026-10-03; spec world-audio-hookin-spec.md §7.3).
    'VEHAUD': 15,   # obj R id key(hex) patch rpm pos("x y z") fwd("x y z") f144 f148 f152 horn skid rel176 listener("x y z")   (traffic; TrafficEngine update sub_824D6478)
    # World audio gap G3 (2026-10-03, audiox): posts per Player sound instance (local rider / NPC skater).
    'PLAYERPOST': 12,  # class via(POST|SPLC|WSTART|WSTOP|BAILGRUNT) object local72 local16 mixkey(hex) rec_id rec_index a b words chain
    # Solver iteration count (2026-10-04, physics): writer 82763E00 and its caller, the frame update 82859E70.
    'ITERSET': 9,   # owner slot(0 lr 8285A1E8, 1 lr 8285A1F8, -1 other) mode(+8) sim before176 after176 calls why(1 change 2 heartbeat 4 first) chain
    'ITERTICK': 7,  # game calls f0 f1 f2 f3 (calls per second by r4 & 3) b145
    # Session-marker returns and the teleport flow (2026-10-04, audiox; hooks_marker.cpp).
    'TPMARK': 7,    # lr distance hold dest("x y z") r5 flags("84 85 86") callers   (teleport request sub_825582D0)
    'TPDEC': 7,     # dest("x y z") r5 r6 flags("r7 r8 r9") load busy callers   (teleport decision sub_82706F50)
    'TPSTREAM': 4,  # dest("x y z") r4 result callers   (streamed-in check sub_82864C40)
    'TPFX': 2,      # object amount   (VisualDirector teleport effect sub_827A9C60)
    'FEREQ': 3,     # key(hex) r4 callers   (front-end sound request sub_825DFAF0)
    'GSTATE': 5,    # machine state_fn r5 r6 callers   (state change sub_826DFB30)
    'GEVENT': 3,    # queue event callers   (state event push sub_826DB648)
    'HUBMSG': 3,    # id r5 callers   (message post sub_82B938D8)
    # Trigger query points and challenge hulls (2026-10-04, world; hooks_world.cpp). Summary: a per-entity script of your own.
    'TRIGQRY': 7,   # group entity A("x y z", overwritten: a unit vector) B=head C=hips centre axis   (sub_82DD80B8)
    'TRIGENT': 8,   # entity vtable fn+4 fn+8 fn+12 fn+16 fn+20 words("+4..+28")   (once per entity vtable)
    'TRIGGRP': 3,   # group id item+216   (AddVolume sub_82DD7668)
    'HULLENTER': 4, # manager entry key pos("x y z")   (dynamic hull manager sub_82D519C0 diff)
    'HULLEXIT': 4,
    'HULLENT': 2,   # manager entity   (sub_82D514C0, on change)
    # Vehicles: skitching, the wipeout vehicle term, light phases, junctions (2026-10-05; hooks_vehicles.cpp, VEHCONN in
    # hooks_traffic.cpp). Categories skitch (SKITCH*, VEHBAIL) and traffic (TRAFLIGHT2, TRAFPROG, VEHJUNC, VEHCONN).
    'HOOKARMED': 5,   # hook lr r3 r4 r5   (once per hook: installed and running)
    'SKITCH': 21,     # event(GRAB|HELD|RELEASE) latch(124 any skitcher|128 id-matched) vehicle f4402 f4403 speed3412 accel3408 cap3688 f3680 f3684 carpos("x y z") carfwd("x y z") p skaterpos("x y z") offset("x y z", car rows 0/1/2) held_ms asserts gaps this id callers
    'SKITCHCALL': 6,  # this r4 r5 handle14992 calls callers   (skitch setter sub_82C361E8)
    'SKITCHST': 12,   # selector p current suggested why(1 bits 2 skitch transition 4 vehicle window) f2476 f2480 value2592 obj2464 sel48 pos("x y z") speed2656   (sub_82D8ADE8)
    'SKITCHCAND': 10, # S p c1 S196 c2 S500 c3 S12768 f2480 hex(S+0..47)   (grab candidates sub_82D740F8)
    'VEHBAIL': 16,    # W p SC where(G|A|?) force4056 limit skitching vehicle_contact skitch_contact req7("before after") count200 cooldown192 g8flag skaterforce4048 g11force4060 f2476   (sub_82D90C98)
    'TRAFPROG': 7,    # index C n phases("kind:len:word,...") n2 phases2 split300   (once per signal controller)
    'TRAFLIGHT2': 14, # index C phase n kind length remaining phase2 n2 kind2 length2 split300 ticks since_ms   (sub_82E158D8, on change)
    'VEHJUNC': 16,    # vehicle caller J seg segdata g84 w88 w92 w96 lights r4 r7 result out("w0 w1 w2") light("phase kind"|-) pos("x y z")   (sub_82E11E90)
    'VEHCONN': 15,    # vehicle why("1 jstate 2 seg 4 lane 8 segid 16 first") jstate("old new") seg("old new") lane("old new") segid4144 id4136 target_lane manoeuvre dist3640 speed3412 accel3408 flags("4402 4403") pos fwd
    'PEDAUD': 21,  # obj S id index count flags("68 69 71 80") feet("73 74") model key(hex) s96 ("88 92 120 124") class132 speech136 mats("140 144") f148 f152 f156 o128 o144 opos("x y z") listener("x y z")   (npc; PedestrianSFX process sub_824D8078)
    # Category aiskater (2026-10-15, hooks_physics.cpp): NPC skater path controller hips-test byte.
    'AIPC922': 5,   # brain pc pc+922 pc+940 brain+168
    # Category menus (2026-10-12, src/research/hooks_menus.cpp): Trick Guide 3D demo camera.
    'TGCAM': 17,    # seq P cam f1 P+108 P+112 P+116 P+121 fov M(16) elem(10) c38(16) b58b(16) b58a(16) icebuf hex
                    # hex(player+368: element words 92..103); runs before 2026-10-12 23:30 have 16 fields
    'TGFIT': 8,     # obj index key holder track len_before len_after icebuf
    'TGPROP': 5,    # track result lr track+8 [track+12]+12
    'TGTAN': 8,     # r3 r4 r5 r6 r7 u16+60 u16+64 lr
    'TGE87': 5,     # r3 r4 ret_r3 ret_f1 lr
    'TGLOC': 2,     # list key
    # Category render (2026-10-09, src/research/hooks_render.cpp): draw order of one frame, g_ViewDotLight writer.
    'DOFRAME': 3,   # start|end|capped seq ms_since_previous_boundary
    'DOLIST': 8,    # seq view list_offset first count first_key last_key callers
    'DOSEEN': 2,    # vtable fence|-
    'DOAPPLY': 11,  # seq tid instance vtable bind depth_i blend_i raster_i technique vs callers
    'VDLWRITE': 8,  # function G before("x y z w") after("x y z w") lr callers tid lines_left
    'VDLVAL': 3,    # G value("x y z w") frames_changed
    # Category cas (2026-10-08, hooks_cas.cpp): skater actor character records / natural stance.
    'CASACTOR': 8,  # actor r4 index(slot) r6 r7 r8 character1804 callers   (actor ctor sub_82590DC0)
    'CASID': 5,     # actor index record_id(u64 hex) name_word name   (sub_824581C0 from the ctor, lr 82591158)
    'CASCHAR': 5,   # actor r5 r6 name_ptr name   (sub_82B973C8 from the ctor, lr 825911E8)
    'CASSET': 10,   # index lr b120 b121 w124 w144 goofy_out style_out style callers   (sub_82590B50; per (index, lr) on change)
    'CASREC': 3,    # index record_addr hex168   (once per index)
}


class Trace:
    def __init__(self, path: str | Path):
        self.path = Path(path)
        self.lines: dict[str, list[tuple[float, list[str]]]] = defaultdict(list)
        self.marks: list[tuple[float, str]] = []
        self.clock: float | None = None  # unix ms at trace ms 0
        self.captures: list[tuple[float, int]] = []
        for raw in self.path.open(encoding='utf-8', errors='replace'):
            f = raw.rstrip('\n').split('\t')
            if len(f) < 2:
                continue
            try:
                ms = float(f[1])
            except ValueError:
                continue
            kind = f[0]
            try:  # a few lines can be split/merged by the two trace writers; skip what doesn't parse
                if kind == 'MARK':
                    self.marks.append((ms, f[2] if len(f) > 2 else ''))
                elif kind == 'CLOCK' and self.clock is None and len(f) > 2:
                    self.clock = int(f[2]) - ms
                elif kind == 'CAPTURE' and len(f) > 2:
                    self.captures.append((ms, int(f[2])))
            except ValueError:
                continue
            self.lines[kind].append((ms, f[2:]))

    def malformed(self) -> dict[str, int]:
        """Lines per kind whose field count differs from FIELD_COUNTS (should be all zero)."""
        return {k: sum(1 for _, f in self.lines.get(k, []) if len(f) != n) for k, n in FIELD_COUNTS.items()}

    def kinds(self) -> dict[str, int]:
        return {k: len(v) for k, v in self.lines.items()}

    def stops(self) -> list[tuple[str, float, float]]:
        """(name, start ms, end ms) for each `at <name>` mark, ending at the next `at`/`done` mark."""
        starts = [(ms, label[3:].split('#')[0].strip()) for ms, label in self.marks if label.startswith('at ')]
        ends = [ms for ms, label in self.marks if label.startswith(('at ', 'done'))]
        out = []
        for ms, name in starts:
            later = [e for e in ends if e > ms]
            out.append((name, ms, later[0] if later else float('inf')))
        return out

    def by_stop(self, kind: str) -> Iterator[tuple[str, list[tuple[float, list[str]]]]]:
        for name, a, b in self.stops():
            yield name, [(ms, f) for ms, f in self.lines.get(kind, []) if a <= ms < b]

    def shot_for(self, ms: float) -> Path | None:
        """Nearest screenshot (`<trace stem>_shots/` or `shots/`, `shot_<unix ms>.jpg|png`) to trace time `ms`."""
        if self.clock is None:
            return None
        # scripted runs: <trace stem>_shots/; passive sessions (PLAY_TRACE_*.bat): shots/
        folders = [self.path.with_name(self.path.stem + '_shots'), self.path.with_name('shots')]
        folder = next((f for f in folders if f.exists()), None)
        shots = sorted(folder.glob('shot_*.*')) if folder else []
        if not shots:
            return None
        stamps = [int(p.stem.split('_')[1]) for p in shots]
        target = self.clock + ms
        i = bisect_left(stamps, target)
        best = min((j for j in (i - 1, i) if 0 <= j < len(stamps)), key=lambda j: abs(stamps[j] - target))
        return shots[best]

    def capture(self, a_ms: float, b_ms: float):
        """Stereo float32 frames of `trace.f32` between two trace times (numpy array (n, 2))."""
        import numpy
        ms = numpy.array([c[0] for c in self.captures])
        frames = numpy.array([c[1] for c in self.captures])
        a, b = (int(numpy.interp(t, ms, frames)) for t in (a_ms, b_ms))
        data = numpy.memmap(self.path.with_suffix('.f32'), dtype='<f4', mode='r')
        return numpy.asarray(data[2 * a:2 * b]).reshape(-1, 2)


if __name__ == '__main__':
    import sys
    t = Trace(sys.argv[1])
    print(t.kinds())
    print('malformed (fixed-layout kinds):', t.malformed())
    for name, a, b in t.stops():
        print(f'{name:28s} {a / 1000:8.1f} .. {b / 1000:8.1f} s')
