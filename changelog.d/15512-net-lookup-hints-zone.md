- **`net.connect` `lookup` hints and zones.** A custom `lookup` now receives
  `dns.ADDRCONFIG` in `hints` when none were given and no family is pinned
  (off Windows), as in Node, and a zoned IPv6 answer such as `fe80::1%lo0`
  connects through that interface instead of without a scope.
