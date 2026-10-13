Built-in prototypes that inherit Object's generic `valueOf` now receive the real thunk instead of a no-op placeholder. Specialized Number, String, and Date valueOf behavior remains in place.
