### Fixed

- Correct `net` named and renamed `Socket` and `Stream` constructor dispatch while preserving local binding precedence.
- Match Node.js for unopened sockets' `bufferSize` and `address()` results and for `cork()` / `uncork()` return values.
- Add a 28-row constructor-route regression covering import forms, renamed imports, and local binding precedence.
