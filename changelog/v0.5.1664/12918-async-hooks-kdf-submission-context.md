### Fixed

- `crypto.pbkdf2` and `crypto.scrypt` callbacks now preserve their submission-time
  `AsyncLocalStorage` stores after the submitting scope has ended. Each pending
  job retains the existing GC-owned context snapshot beside its callback in the
  existing pool job roots. Owner-thread delivery reads the rewritten roots and
  uses the existing throw-safe context entry and restoration around allocation
  and scheduling. No new context store or GC mechanism is introduced, and
  Argon2 delivery is unchanged.
- A public regression covers four simultaneously pending jobs: two PBKDF2 and
  two scrypt submissions with distinct store labels, 16-byte results, and an
  `undefined` store outside the submitting scopes. The original reproducer and
  this regression failed on the baseline and passed on the candidate; previously
  passing controls were preserved.

This is a bounded async-context correction, not complete `async_hooks` parity.
Remaining baseline failures, the missing zlib archive in the eight-package
verification bundle, and 18 safety-excluded cases that were not run remain
outside this fix.
