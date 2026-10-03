"""Cross-reference helper for a big-endian PowerPC memory image of the game (the Xbox 360 executable's
loaded image, which you dump yourself from your own copy; base address 0x82000000 by default).

usage:
  py -3.13 tools/recomp-code-search/ppcxref.py --image IMG --funcs FUNCS addr  ADDR [ADDR ...]   # code that builds the address (lis + addi/load/store)
  py -3.13 tools/recomp-code-search/ppcxref.py --image IMG --funcs FUNCS str   TEXT [TEXT ...]   # find strings, then xref each hit
  py -3.13 tools/recomp-code-search/ppcxref.py --image IMG --funcs FUNCS calls ADDR [ADDR ...]   # callers (bl) of a function
  py -3.13 tools/recomp-code-search/ppcxref.py --image IMG --funcs FUNCS func  ADDR [ADDR ...]   # containing function start
  py -3.13 tools/recomp-code-search/ppcxref.py --image IMG --funcs FUNCS ptr   ADDR [ADDR ...]   # data words equal to the address
                                                                                                    # (vtables, binding tables)
Options: --base (image base, default 0x82000000), --code LO-HI (code range to scan, hex; default the whole image).
Environment: PPC_IMAGE, PPC_FUNCS stand in for --image / --funcs.

FUNCS lists function starts, one hex address first on each line. Build it from your recomp's generated
sources:
  cd <skate3recomp>/generated && grep -o "DEFINE_REX_FUNC(sub_[0-9A-F]*)" skate3_recomp.*.cpp \\
    | sed 's/:DEFINE_REX_FUNC(sub_/ /;s/)//' | awk '{print $2, $1}' | sort > funcs.txt
Heuristic: a lis value is valid for 48 instructions in the same function.
"""
import argparse
import bisect
import os
import re
import struct
import sys
from pathlib import Path

IMG = b''
BASE = 0x82000000
CODE_LO = CODE_HI = 0
FUNCS = []

# D-form opcodes whose rA+simm is an address: addi 14, lwz 32, lwzu 33, lbz 34, stw 36, stb 38, lhz 40,
# sth 44, lfs 48, lfd 50, stfs 52, stfd 54, ld/ldu 58, std 62
DFORM = {14, 32, 33, 34, 35, 36, 37, 38, 39, 40, 42, 44, 48, 50, 52, 54, 58, 62}


def func_of(a):
    i = bisect.bisect_right(FUNCS, a) - 1
    return FUNCS[i] if i >= 0 else 0


_W = None


def W():
    global _W
    if _W is None:
        n = (CODE_HI - CODE_LO) // 4
        _W = struct.unpack_from('>%dI' % n, IMG, CODE_LO - BASE)
    return _W


def xrefs(targets, slack=0):
    """targets: addresses (or (lo, hi) ranges). Returns a list of (pc, ea, func)."""
    w = W()
    out = []
    ranges = [(t, t + slack) if isinstance(t, int) else t for t in targets]
    lis = {}
    fstarts = set(FUNCS)
    for i, ins in enumerate(w):
        pc = CODE_LO + 4 * i
        if pc in fstarts:
            lis.clear()
        op = ins >> 26
        rd = (ins >> 21) & 31
        ra = (ins >> 16) & 31
        imm = ins & 0xFFFF
        simm = imm - 0x10000 if imm & 0x8000 else imm
        if op == 15 and ra == 0:  # lis
            lis[rd] = ((simm << 16) & 0xFFFFFFFF, i)
            continue
        if op in DFORM and ra in lis and i - lis[ra][1] < 48:
            d = simm & ~3 if op in (58, 62) else simm
            ea = (lis[ra][0] + d) & 0xFFFFFFFF
            for lo, hi in ranges:
                if lo <= ea <= hi:
                    out.append((pc, ea, func_of(pc)))
                    break
        if op not in (14, 15) and rd in lis and op in (14, 32, 33, 34, 40, 58, 31):
            lis.pop(rd, None)  # destination overwritten
    return out


def callers(target):
    out = []
    for i, ins in enumerate(W()):
        if ins >> 26 == 18 and ins & 1:  # bl
            li = ins & 0x03FFFFFC
            if li & 0x02000000:
                li -= 0x04000000
            pc = CODE_LO + 4 * i
            t = li if ins & 2 else pc + li
            if t & 0xFFFFFFFF == target:
                out.append((pc, func_of(pc)))
    return out


def find_str(s):
    return [m.start() + BASE for m in re.finditer(re.escape(s.encode('latin1')), IMG)]


def cstr(a):
    o = a - BASE
    return IMG[o:IMG.index(b'\0', o)].decode('latin1')


def ptrs(a):
    b = struct.pack('>I', a)
    return [m.start() + BASE for m in re.finditer(re.escape(b), IMG) if m.start() % 4 == 0]


def main():
    global IMG, BASE, CODE_LO, CODE_HI, FUNCS
    parser = argparse.ArgumentParser(description='Cross-reference helper for a big-endian PowerPC memory image.',
                                     epilog=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('cmd', choices=('addr', 'str', 'calls', 'func', 'ptr'))
    parser.add_argument('args', nargs='+')
    parser.add_argument('--image', default=os.environ.get('PPC_IMAGE'), help='memory image file')
    parser.add_argument('--funcs', default=os.environ.get('PPC_FUNCS'), help='function start list')
    parser.add_argument('--base', default='0x82000000', help='image base address (hex)')
    parser.add_argument('--code', help='code range LO-HI (hex); default the whole image')
    a = parser.parse_args()
    if not a.image or not a.funcs:
        parser.error('--image and --funcs (or PPC_IMAGE / PPC_FUNCS) are required')
    IMG = Path(a.image).read_bytes()
    BASE = int(a.base, 16)
    if a.code:
        lo, hi = a.code.split('-')
        CODE_LO, CODE_HI = int(lo, 16), int(hi, 16)
    else:
        CODE_LO, CODE_HI = BASE, BASE + len(IMG) // 4 * 4
    FUNCS = sorted(int(l.split()[0], 16) for l in Path(a.funcs).read_text().splitlines() if l.strip())
    if a.cmd == 'addr':
        for pc, ea, f in xrefs([int(x, 16) for x in a.args]):
            print(f'{pc:08X} -> {ea:08X} in sub_{f:08X}')
    elif a.cmd == 'str':
        starts = set()
        for s in a.args:
            for h in find_str(s):
                o = h - BASE
                while o > 0 and 0x20 <= IMG[o - 1] < 0x7f:
                    o -= 1
                starts.add(o + BASE)
        res = xrefs(sorted(starts))
        for st in sorted(starts):
            print(f'{st:08X} "{cstr(st)[:80]}"')
            for pc, ea, f in res:
                if ea == st:
                    print(f'    {pc:08X} in sub_{f:08X}')
    elif a.cmd == 'calls':
        for x in a.args:
            for pc, f in callers(int(x, 16)):
                print(f'{pc:08X} in sub_{f:08X}')
    elif a.cmd == 'func':
        for x in a.args:
            print(f'sub_{func_of(int(x, 16)):08X}')
    elif a.cmd == 'ptr':
        for x in a.args:
            addr = int(x, 16)
            for p in ptrs(addr):
                nb = struct.unpack_from('>4I', IMG, p - BASE - 4)
                print(f'{addr:08X} @ {p:08X}  [-4..+12] ' + ' '.join(f'{v:08X}' for v in nb))


if __name__ == '__main__':
    sys.exit(main())
