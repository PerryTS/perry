- Windows UI apps now wake immediately for runtime work, wait against the
  nearest JavaScript timer deadline, and drive `onFrame` from DWM composition
  boundaries. Timer/promise latency and frame cadence are no longer gated by
  the 50 ms maintenance heartbeat; posted work and frame messages are
  coalesced to keep async-heavy applications from flooding the Win32 queue.
