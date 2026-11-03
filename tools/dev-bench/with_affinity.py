"""Run a command pinned to a CPU mask (children inherit it), e.g. to bench the setup as a smaller machine.

    py -3.13 with_affinity.py 0xFFFF <command> [args...]

0xFFFF on the desktop = its 8 P-cores with hyperthreading: about the laptop's 8C/16T (doc 08 "Customiser worker share").
"""
import ctypes, subprocess, sys

mask = int(sys.argv[1], 0)
kernel32 = ctypes.windll.kernel32
kernel32.GetCurrentProcess.restype = ctypes.c_void_p
kernel32.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
if not kernel32.SetProcessAffinityMask(kernel32.GetCurrentProcess(), mask):
    raise SystemExit(f'SetProcessAffinityMask({mask:#x}) failed')
raise SystemExit(subprocess.call(sys.argv[2:]))
