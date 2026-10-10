#!/usr/bin/env python3
"""Snapshot an owned x86-64 child at its stdout write, outside perf runs."""
import ctypes, json, os, signal, sys
from pathlib import Path

FIELDS = 'r15 r14 r13 r12 rbp rbx r11 r10 r9 r8 rax rcx rdx rsi rdi orig_rax rip cs eflags rsp ss fs_base gs_base ds es fs gs'.split()
class Registers(ctypes.Structure):
    _fields_ = [(name, ctypes.c_ulonglong) for name in FIELDS]

libc = ctypes.CDLL(None, use_errno=True)
libc.ptrace.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p]
libc.ptrace.restype = ctypes.c_long
def trace(request, pid, data=0):
    if libc.ptrace(request, pid, None, data) == -1:
        raise OSError(ctypes.get_errno(), 'ptrace')

def snapshot(pid):
    totals = {}
    category = ''
    for line in Path(f'/proc/{pid}/smaps').read_text().splitlines():
        if '-' in line.split()[0]:
            fields = line.split(maxsplit=5)
            path = fields[5] if len(fields) > 5 else 'anonymous'
            category = 'executable' if path == executable else ('file' if path.startswith('/') else 'anonymous')
        elif line.startswith(('Rss:', 'Anonymous:', 'AnonHugePages:')):
            key, value, _ = line.split()
            name = category + '.' + key[:-1]
            totals[name] = totals.get(name, 0) + int(value)
    totals['status'] = '\n'.join(line for line in Path(f'/proc/{pid}/status').read_text().splitlines()
                                 if line.startswith(('VmHWM:', 'VmRSS:', 'THP_enabled:')))
    return totals

dest, executable, *args = sys.argv[1:]
executable = str(Path(executable).resolve())
pid = os.fork()
if pid == 0:
    trace(0, 0)  # Child requests tracing, then executes only the requested arm.
    os.execv(executable, [executable, *args])

def timeout(*_):
    os.kill(pid, signal.SIGKILL)
    raise TimeoutError('owned child exceeded 120 seconds')
signal.signal(signal.SIGALRM, timeout)
signal.alarm(120)
try:
    os.waitpid(pid, 0)
    trace(0x4200, pid, 1)  # PTRACE_O_TRACESYSGOOD
    entering = True
    rows = []
    while True:
        trace(24, pid)  # PTRACE_SYSCALL
        _, status = os.waitpid(pid, 0)
        if os.WIFEXITED(status) or os.WIFSIGNALED(status):
            break
        if os.WSTOPSIG(status) != (signal.SIGTRAP | 0x80):
            raise RuntimeError(f'unexpected child stop {status}')
        regs = Registers()
        trace(12, pid, ctypes.byref(regs))
        if entering and regs.orig_rax == 1 and regs.rdi == 1:
            rows.append(dict(phase='stdout', **snapshot(pid)))
        if entering and regs.orig_rax in [60, 231]:
            rows.append(dict(phase='exit', **snapshot(pid)))
        entering = not entering
    Path(dest).write_text(json.dumps(rows, indent=2))
finally:
    signal.alarm(0)
