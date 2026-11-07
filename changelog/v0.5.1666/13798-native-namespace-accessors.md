### Fixed

- Read native namespace own accessors through their getter instead of exposing internal AccessorPair storage. Preserve receiver and exception identity, getterless undefined values, data override ordering and reflection without getter invocation.
