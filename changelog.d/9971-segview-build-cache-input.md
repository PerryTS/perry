### Fixed

- Register the segment-view lowering switch as a build-cache input so changing
  `PERRY_SEGVIEW` cannot reuse a binary emitted under the opposite setting.
  `PERRY_SEGVIEW_DIAG` remains diagnostic-only and is explicitly excluded from
  the cache key.
