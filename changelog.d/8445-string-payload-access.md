- Added GC-safe owned-copy and rooted-reread APIs for runtime string payloads,
  plus a CI ratchet that prevents new open-coded `StringHeader` byte access.
