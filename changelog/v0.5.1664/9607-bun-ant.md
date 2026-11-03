### Bun compatibility

- **The opt-in Bun platform now exposes Claude Code's vendor `Bun.ant`
  hooks.** `getPeerUid(fd)` and `getPeerPid(fd)` read local-socket peer
  credentials without taking ownership of the descriptor and return `null`
  for invalid, closed, or unsupported descriptors. `memoryPressureLevel()`
  reports `"normal"`, `"warning"`, `"critical"`, or `null` through a
  platform-specific pressure source. Perry's `node:net` socket facade also
  supplies the private `_handle.fd` bridge used by Claude Code.
