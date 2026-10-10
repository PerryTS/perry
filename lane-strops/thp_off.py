#!/usr/bin/env python3
"""Disable THP for this process and exec the measurement, without global writes."""
import ctypes, os, sys
libc = ctypes.CDLL(None, use_errno=True)
if libc.prctl(41, 1, 0, 0, 0) != 0: raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE')
os.execvp(sys.argv[1], sys.argv[1:])
