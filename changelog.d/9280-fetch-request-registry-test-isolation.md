### Fixed

- **`perry-ext-fetch`'s request guard-release regression test no longer flakes under parallel `cargo-test` runs (#9267).** The request-registry unit tests now share a test-only isolation mutex, so the probe's `REQUEST_HANDLES.try_lock()` cannot mistake a sibling's legitimate access for a guard leaked by the reader under test. Production storage and locking are unchanged. The probe still catches a registry guard that remains held after a reader returns, and now distinguishes that failure from mutex poisoning instead of blaming a named reader for either condition.
