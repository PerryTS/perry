### Changed

- Loop-called arrows held in immutable bindings — captured, module-global, or parameter — are resolved once at body entry and dispatched directly, instead of paying the full `js_closure_callN` dispatcher on every call. The isolated captured-arrow call drops from 8.0 to 4.4 ns/op; a resolution that fails (non-arrow, bound function, reassigned binding, pre-initialization sentinel) keeps the exact dispatcher fallback per call.
