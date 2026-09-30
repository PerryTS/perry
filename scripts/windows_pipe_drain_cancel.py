"""Native witness: destroy releases an unread pipe drain while JS stays alive."""
import ctypes
from ctypes import wintypes as W
import os
from pathlib import Path
import subprocess
import time


def check(compiler, env, directory):
    """Keep the server's bytes unread and inspect whether its client has closed."""
    source = Path(directory) / "pipe-cancel-live.ts"
    source.write_text("""import net from 'node:net';
const c = net.connect(process.env.AUDIT_PIPE);
c.on('error', () => {});
c.on('connect', () => {
  c.end('unread bytes');
  setTimeout(() => c.destroy(), 100);
});
c.on('close', () => console.log('closed'));
setTimeout(() => console.log('still alive'), 4000);
""", encoding="utf-8")
    exe = Path(directory) / "pipe-cancel-live.exe"
    subprocess.run([str(compiler), "compile", str(source), "--no-auto-optimize",
                    "-o", str(exe)], env=env, capture_output=True, check=True, timeout=120)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    native = ctypes.WinDLL("ntdll")
    kernel.CreateNamedPipeW.argtypes = [W.LPCWSTR, W.DWORD, W.DWORD, W.DWORD,
                                      W.DWORD, W.DWORD, W.DWORD, ctypes.c_void_p]
    kernel.CreateNamedPipeW.restype = W.HANDLE
    kernel.ConnectNamedPipe.argtypes = [W.HANDLE, ctypes.c_void_p]
    kernel.CloseHandle.argtypes = [W.HANDLE]

    class Overlapped(ctypes.Structure):
        _fields_ = [("Internal", ctypes.c_size_t), ("InternalHigh", ctypes.c_size_t),
                    ("Offset", W.DWORD), ("OffsetHigh", W.DWORD), ("hEvent", W.HANDLE)]

    class Status(ctypes.Structure):
        _fields_ = [("Status", ctypes.c_size_t), ("Information", ctypes.c_size_t)]

    native.NtQueryInformationFile.argtypes = [W.HANDLE, ctypes.c_void_p,
                                             ctypes.c_void_p, W.ULONG, W.ULONG]
    native.NtQueryInformationFile.restype = ctypes.c_long
    child_env = dict(env, AUDIT_PIPE=rf"\\.\pipe\perry-drain-cancel-{os.getpid()}")
    # Duplex, overlapped, byte-mode pipe. Nothing reads the buffered request.
    pipe = kernel.CreateNamedPipeW(child_env["AUDIT_PIPE"], 3 | 0x40000000,
                                  0, 1, 4096, 4096, 0, None)
    if pipe == W.HANDLE(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    process = None
    overlapped = Overlapped()  # Retained until connection/handle cleanup completes.
    try:
        if not kernel.ConnectNamedPipe(pipe, ctypes.byref(overlapped)):
            if ctypes.get_last_error() != 997:  # ERROR_IO_PENDING
                raise ctypes.WinError(ctypes.get_last_error())
        process = subprocess.Popen([str(exe)], env=child_env, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, encoding="utf-8")
        deadline = time.monotonic() + 3
        while True:
            info = (W.ULONG * 10)()  # FILE_PIPE_LOCAL_INFORMATION
            status = Status()
            result = native.NtQueryInformationFile(pipe, ctypes.byref(status),
                                                   ctypes.byref(info), ctypes.sizeof(info), 24)
            if result != 0:
                raise RuntimeError(f"Pipe query failed: {result:#x}")
            # State 4 is closing; state 3 means a duplicate still retains the client.
            if info[8] == 4 and info[5] > 0 and process.poll() is None:
                break
            if time.monotonic() >= deadline or process.poll() is not None:
                raise RuntimeError(f"Drain retained the client: pipe state {info[8]}, bytes {info[5]}")
            time.sleep(0.01)
        stdout, stderr = process.communicate(timeout=6)
        if process.returncode != 0 or stdout.splitlines() != ["closed", "still alive"]:
            raise RuntimeError(f"Cancellation probe failed: {stdout!r} {stderr!r}")
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.communicate()
        kernel.CloseHandle(pipe)
    print("PASS unread pipe drain cancellation releases its native handle", flush=True)
