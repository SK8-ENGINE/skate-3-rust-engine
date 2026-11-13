"""How much of the machine setup may use: worker counts and process priority.

Setup should finish fast but leave the PC usable (user, 2026-09-30): bounded
worker counts, below-normal priority for the conversion processes, and
overrides for people who want more or less.

Every count comes from one budget, `cores()`: 75% of the logical CPUs this process may run on (user 2026-10-24:
"uses 75% of the cores leaving headroom for the user"), at least 1.

Overrides (environment variables, read when setup starts the work):
- SKATE_SETUP_CORES=<n>        the budget itself
- SKATE_SETUP_MAP_WORKERS=<n>  maps converted at once (default: see install.map_workers: up to 3, half the budget,
                               memory permitting)
- SKATE_SETUP_THREADS=<n>      threads inside one map job (default: the budget left after the customiser, over 3 jobs)
- SKATE_SETUP_DECODE_WORKERS=<n>  audio banks / beds decoded at once (default: a quarter of the budget; the map
                               jobs run beside the audio)
- SKATE_SETUP_CUSTOMISER_WORKERS=<n>  customiser processes beside the maps (default: a quarter of the budget, 1..8)
- SKATE_SETUP_EXTRACT_THREADS=<n> archive entries read and written at once (default: the budget, at most 8)
- SKATE_SETUP_PRIORITY=below_normal|normal|idle   (default below_normal)

Nothing here changes what setup writes, only how many things run at once and
at which priority. This module's name matches no pipeline fingerprint glob on
purpose: changing a budget must not rebuild anyone's assets.
"""
import os

BELOW_NORMAL_PRIORITY_CLASS = 0x00004000
IDLE_PRIORITY_CLASS = 0x00000040
NORMAL_PRIORITY_CLASS = 0x00000020
PRIORITIES = {'below_normal': BELOW_NORMAL_PRIORITY_CLASS, 'normal': NORMAL_PRIORITY_CLASS,
              'idle': IDLE_PRIORITY_CLASS}


def _positive(name, environ=None):
    value = (os.environ if environ is None else environ).get(name, '').strip()
    if not value:
        return None
    try:
        number = int(value)
    except ValueError:
        return None
    return number if number >= 1 else None


def _cpu_count():
    if os.name == 'nt':
        # Python's process_cpu_count ignores the affinity on Windows.
        import ctypes
        kernel32 = ctypes.windll.kernel32
        kernel32.GetCurrentProcess.restype = ctypes.c_void_p
        kernel32.GetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t)]
        mask, system = ctypes.c_size_t(), ctypes.c_size_t()
        if kernel32.GetProcessAffinityMask(kernel32.GetCurrentProcess(), ctypes.byref(mask), ctypes.byref(system)) and mask.value:
            return bin(mask.value).count('1')
    counter = getattr(os, 'process_cpu_count', None) or os.cpu_count
    return counter() or 1


def cores(cpu_count=None, environ=None):
    """The CPUs setup may keep busy: 75% of the ones this process may run on (its affinity), less what other programs
    were using when setup started (`measure_load`), at least a quarter of them."""
    override = _positive('SKATE_SETUP_CORES', environ)
    if override:
        return override
    measured = _positive('SKATE_SETUP_MEASURED_CORES', environ) if cpu_count is None else None
    if measured:
        return measured
    return max(1, (cpu_count or _cpu_count()) * 3 // 4)


def _system_times():
    """(idle, total) CPU time of the whole machine (Windows GetSystemTimes), or None."""
    if os.name != 'nt':
        return None
    import ctypes
    idle, kernel, user = (ctypes.c_ulonglong() for _ in range(3))
    if not ctypes.windll.kernel32.GetSystemTimes(ctypes.byref(idle), ctypes.byref(kernel), ctypes.byref(user)):
        return None
    return idle.value, kernel.value + user.value  # kernel time includes idle time


def load_budget(cpu_count, busy):
    """The budget for a machine whose other programs keep `busy` (0..1) of its CPUs busy (user 2026-10-24: gauge the
    threads from how busy the host is): 75% minus the busy CPUs, never under a quarter."""
    return max(max(1, cpu_count // 4), cpu_count * 3 // 4 - int(cpu_count * busy))


def measure_load(report=None, seconds=0.5, environ=None):
    """Sample the machine's CPU load once when setup starts and fix the budget for the whole setup (its child
    processes inherit SKATE_SETUP_MEASURED_CORES). No-op with SKATE_SETUP_CORES or an earlier measurement."""
    environ = os.environ if environ is None else environ
    if _positive('SKATE_SETUP_CORES', environ) or _positive('SKATE_SETUP_MEASURED_CORES', environ):
        return
    import time
    first = _system_times()
    if first is None:
        return
    time.sleep(seconds)
    second = _system_times()
    total = second[1] - first[1]
    busy = 1 - (second[0] - first[0]) / total if total > 0 else 0.0
    count = _cpu_count()
    budget = load_budget(count, min(1.0, max(0.0, busy)))
    environ['SKATE_SETUP_MEASURED_CORES'] = str(budget)
    if report:
        report(f'CPU budget: {budget} of {count} logical CPUs (other programs used {busy:.0%})')


def customiser_workers(cpu_count=None, environ=None):
    """Customiser worker processes while the maps convert beside them."""
    override = _positive('SKATE_SETUP_CUSTOMISER_WORKERS', environ)
    if override:
        return min(override, 32)
    return max(1, min(8, cores(cpu_count, environ) // 4))


MAPS_DONE = 'SKATE_SETUP_MAPS_DONE'
WORKER_MEMORY = 2 * 1024**3


def customiser_limit(workers, running=0, environ=None):
    """How many customiser workers may run now: `workers` while the maps convert, then the cores the maps released
    (the budget less one for the setup itself), as far as free memory allows beside the `running` ones (doc 08
    "Customiser worker share").
    The maps-done marker is a file install.py names in SKATE_SETUP_MAPS_DONE and creates when the map jobs end."""
    environ = os.environ if environ is None else environ
    marker = environ.get(MAPS_DONE)
    if _positive('SKATE_SETUP_CUSTOMISER_WORKERS', environ) or not marker or not os.path.exists(marker):
        return workers
    from .install import available_memory
    available = available_memory()
    spare = 32 if available is None else max(0, (available - 2 * 1024**3) // WORKER_MEMORY)
    return max(workers, min(16, cores(environ=environ) - 1, max(workers, running) + spare))


def extract_threads(cpu_count=None, environ=None):
    """Archive entries read, decompressed and written at once (`BigArchive.extract_entries`)."""
    override = _positive('SKATE_SETUP_EXTRACT_THREADS', environ)
    if override:
        return min(override, 32)
    return max(1, min(8, cores(cpu_count, environ)))


def map_workers(default, environ=None):
    """`SKATE_SETUP_MAP_WORKERS` when set to a positive integer, else `default`."""
    return _positive('SKATE_SETUP_MAP_WORKERS', environ) or default


def job_threads(cpu_count=None, environ=None):
    """Threads one map job may use for zlib/texture compression (zlib releases the GIL): the budget left after the
    customiser's workers, shared by up to three map jobs."""
    override = _positive('SKATE_SETUP_THREADS', environ)
    if override:
        return min(override, 32)
    left = cores(cpu_count, environ) - customiser_workers(cpu_count, environ)
    return max(1, min(32, left // 3))


def decode_workers(cpu_count=None, environ=None):
    """Audio decodes run at once (each a vgmstream process; the writes stay in order)."""
    override = _positive('SKATE_SETUP_DECODE_WORKERS', environ)
    if override:
        return min(override, 32)
    return max(1, min(32, cores(cpu_count, environ) // 4))


def priority_class(environ=None):
    """Windows priority class for setup's conversion work (below normal by default)."""
    value = (os.environ if environ is None else environ).get('SKATE_SETUP_PRIORITY', '').strip().lower()
    return PRIORITIES.get(value, BELOW_NORMAL_PRIORITY_CLASS)


class lowered_priority:
    """Runs the block with this process at `priority_class()`; child processes
    inherit below-normal and idle classes. Restores the previous class after.
    A no-op outside Windows or when the class cannot be read or set."""

    def __init__(self, environ=None):
        self.target = priority_class(environ)
        self.previous = None

    def __enter__(self):
        if os.name != 'nt':
            return self
        import ctypes
        kernel32 = ctypes.windll.kernel32
        kernel32.GetCurrentProcess.restype = ctypes.c_void_p
        kernel32.GetPriorityClass.argtypes = [ctypes.c_void_p]
        kernel32.SetPriorityClass.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        process = kernel32.GetCurrentProcess()
        previous = kernel32.GetPriorityClass(process)
        if previous and previous != self.target and kernel32.SetPriorityClass(process, self.target):
            self.previous = previous
        return self

    def __exit__(self, *_):
        if self.previous is not None:
            import ctypes
            kernel32 = ctypes.windll.kernel32
            kernel32.SetPriorityClass(kernel32.GetCurrentProcess(), self.previous)
            self.previous = None
        return False
