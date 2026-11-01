Park single-agent WASIp3 commands through the SDK's canonical monotonic-clock
wait instead of the default libc's unsupported futex/condvar operation. Use
the same clock wait for blocking sleeps and the spin throttle, without enabling
experimental component threads.
